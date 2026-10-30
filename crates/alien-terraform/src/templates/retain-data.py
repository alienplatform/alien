#!/usr/bin/env python3
"""Detach the generated retention inventory, keeping a recovery receipt.

AWS Aurora and its network are excluded by the generator and remain managed
for destroy. KMS keys remain in the inventory.
"""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile


def tf(*args):
    return subprocess.check_output([CLI, *args], text=True)


def save(path, value):
    # Publish only a complete, durable record, without replacing an earlier one.
    # An interrupted write must not leave an empty intent that cannot be recovered.
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            json.dump(value, output, indent=2)
            output.flush()
            os.fsync(output.fileno())
        os.link(temporary, path)
        descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)
    finally:
        if temporary is not None:
            temporary.unlink()



def protect_dynamodb_ttl(state, selected):
    """Use the installer's AWS credentials; never detach while TTL can still run."""
    addresses = {item['address'] for item in selected}
    for resource in state.get('resources', []):
        if resource.get('mode') != 'managed' or resource.get('type') != 'aws_dynamodb_table':
            continue
        base = '.'.join(filter(None, [resource.get('module'), resource['type'], resource['name']]))
        for instance in resource.get('instances', []):
            suffix = '[' + json.dumps(instance['index_key']) + ']' if 'index_key' in instance else ''
            if base + suffix not in addresses:
                continue
            attributes = instance.get('attributes', {})
            arn = attributes.get('arn', '')
            match = re.fullmatch(r'arn:(aws(?:-us-gov|-cn)?):dynamodb:([a-z0-9-]+):([0-9]{12}):table/([a-zA-Z0-9_.-]+)', arn)
            if not match or attributes.get('id') != match[4]:
                raise RuntimeError('Refresh the exact DynamoDB table ARN before retention; nothing detached')
            def aws(operation, *arguments):
                try:
                    result = subprocess.check_output(
                        ['aws', 'dynamodb', operation, '--region', match[2], '--table-name', arn,
                         *arguments, '--output', 'json', '--no-cli-pager'], text=True)
                except (FileNotFoundError, subprocess.CalledProcessError) as error:
                    raise RuntimeError('DynamoDB retention requires AWS CLI with the installer credentials and DescribeTable, DescribeTimeToLive and UpdateTimeToLive permissions; nothing detached') from error
                return json.loads(result)
            observed = aws('describe-table').get('Table', {})
            if observed.get('TableArn') != arn:
                raise RuntimeError('DynamoDB account/table identity changed; nothing detached')
            ttl = aws('describe-time-to-live').get('TimeToLiveDescription', {})
            if ttl.get('TimeToLiveStatus') == 'ENABLED':
                attribute = ttl.get('AttributeName')
                if not isinstance(attribute, str) or not attribute:
                    raise RuntimeError('DynamoDB TTL attribute is unknown; nothing detached')
                aws('update-time-to-live', '--time-to-live-specification',
                    json.dumps({'Enabled': False, 'AttributeName': attribute}))
                ttl = aws('describe-time-to-live').get('TimeToLiveDescription', {})
            if ttl.get('TimeToLiveStatus') != 'DISABLED':
                raise RuntimeError('DynamoDB TTL is not confirmed DISABLED. Wait for its transition and rerun this retention command; nothing detached')


def installed_inventory(state, module):
    markers = [resource for resource in state.get('resources', [])
               if resource.get('mode') == 'managed'
               and resource.get('type') == 'terraform_data'
               and resource.get('name') == 'retention_inventory'
               and resource.get('module', '') == module]
    if len(markers) != 1 or len(markers[0].get('instances', [])) != 1:
        raise RuntimeError('Installed retention inventory is unavailable. Upgrade the original resource configuration in place before whole-setup teardown; nothing detached')
    instance = markers[0]['instances'][0]
    inventory = instance.get('attributes', {}).get('input', {}).get('value')
    if (instance.get('deposed') or not isinstance(inventory, dict)
            or inventory.get('version') != 1
            or not isinstance(inventory.get('addresses'), list)
            or not all(isinstance(address, str) for address in inventory['addresses'])):
        raise RuntimeError('Invalid installed retention inventory; nothing detached')
    return inventory['addresses']


def verify_installed_inventory(state, manifest, module):
    if installed_inventory(state, module) != manifest:
        raise RuntimeError('Generated manifest does not match the installed retention inventory; nothing detached')


def azure_expiry_guard(state, selected):
    """Only delete actions that may affect detached blob data block retention."""
    accounts = set()
    containers = {}
    for resource in state.get('resources', []):
        if resource.get('mode') != 'managed':
            continue
        base = '.'.join(filter(None, [resource.get('module'), resource['type'], resource['name']]))
        for instance in resource.get('instances', []):
            suffix = '[' + json.dumps(instance['index_key']) + ']' if 'index_key' in instance else ''
            if base + suffix not in selected:
                continue
            attributes = instance['attributes']
            if resource['type'] == 'azurerm_storage_account':
                accounts.add(attributes['id'].lower())
            elif resource['type'] == 'azurerm_storage_container':
                account = attributes.get('storage_account_id')
                name = attributes.get('name')
                if not account:
                    # AzureRM keeps storage_account_id null when the installed
                    # resource uses storage_account_name. Its resource_manager_id
                    # still contains the complete account and container identity.
                    identity = re.fullmatch(
                        r'(/subscriptions/[^/]+/resourceGroups/[^/]+/providers/Microsoft\.Storage/storageAccounts/[^/]+)/blobServices/default/containers/([^/]+)',
                        attributes.get('resource_manager_id') or '', re.IGNORECASE)
                    if (identity and identity[2] == name
                            and identity[1].rsplit('/', 1)[1].lower()
                            == (attributes.get('storage_account_name') or '').lower()):
                        account = identity[1]
                if not account or not name:
                    raise RuntimeError('Cannot identify retained Azure container account/name; refresh or upgrade the installed package before detachment')
                containers.setdefault(account.lower(), set()).add(name)
    for resource in state.get('resources', []):
        if resource.get('mode') != 'managed' or resource['type'] != 'azurerm_storage_management_policy':
            continue
        for instance in resource.get('instances', []):
            attributes = instance['attributes']
            account = attributes.get('storage_account_id', '').lower()
            if account not in accounts and account not in containers:
                continue
            for rule in attributes.get('rule', []):
                if not rule.get('enabled', True):
                    continue
                filters = rule.get('filters') or []
                prefixes = [prefix for entry in filters for prefix in entry.get('prefix_match') or []]
                if (account not in accounts and prefixes
                        and not any(prefix.startswith(name + '/') or (name + '/').startswith(prefix)
                                    for name in containers[account] for prefix in prefixes)):
                    continue
                # Provider state uses -1/null for unset age fields. Tag filters
                # cannot prove safety: retained blobs may acquire matching tags.
                actions = rule.get('actions')
                if not isinstance(actions, list) or not actions:
                    raise RuntimeError('Unknown applicable Azure lifecycle action; nothing detached')
                for action in actions:
                    for kind, blocks in action.items():
                        if kind not in ('base_blob', 'snapshot', 'version'):
                            raise RuntimeError('Unknown applicable Azure lifecycle action; nothing detached')
                        for block in blocks or []:
                            if any(key.startswith('delete') and value is not None and value != -1
                                   for key, value in block.items()):
                                raise RuntimeError('Disable Azure storage lifecycle delete rules affecting retained data, apply; nothing detached')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--detach', action='store_true', help='confirm detachment before whole-setup teardown')
    parser.add_argument('--module-address', default='', help='exact installed child module address')
    parser.add_argument('--receipt-dir', type=Path, required=True, help='private recovery directory outside the module')
    args = parser.parse_args()
    if os.name != "posix":
        raise RuntimeError("Use Linux, macOS, or a WSL Linux filesystem for private recovery receipts; native Windows is not supported")
    manifest_path = Path(__file__).with_name('retained-data.json')
    manifest = json.loads(manifest_path.read_text())
    if not isinstance(manifest, list) or not all(isinstance(address, str) for address in manifest):
        raise RuntimeError('Invalid recovery inventory; nothing detached')
    prefix = args.module_address + '.' if args.module_address else ''
    addresses = {prefix + address for address in manifest}
    # A failed read must abort, never masquerade as an empty installation.
    state = json.loads(tf('state', 'pull'))
    if state.get('version') != 4 or not state.get('lineage') or not isinstance(state.get('serial'), int):
        raise RuntimeError('Unsupported or incomplete Terraform state; nothing detached')
    verify_installed_inventory(state, manifest, args.module_address)
    selected = []
    for resource in state.get('resources', []):
        if resource.get('mode') != 'managed':
            continue
        base = '.'.join(filter(None, [resource.get('module'), resource['type'], resource['name']]))
        if base not in addresses:
            continue
        for instance in resource.get('instances', []):
            suffix = '[' + json.dumps(instance['index_key']) + ']' if 'index_key' in instance else ''
            resource_id = instance['attributes'].get('id')
            if not resource_id:
                raise RuntimeError('Cannot detach a resource without a recovery ID: ' + base + suffix)
            if resource['type'] == 'google_storage_bucket':
                rules = instance['attributes'].get('lifecycle_rule') or []
                if any(action.get('type') == 'Delete' for rule in rules for action in rule.get('action', [])):
                    raise RuntimeError('Remove Cloud Storage delete lifecycle rules and apply before retaining ' + base + suffix + '; nothing detached')
            if resource['type'] == 'azurerm_storage_container':
                if instance['attributes'].get('container_access_type') != 'private':
                    raise RuntimeError('Set Azure container publicRead=false and apply before retaining ' + base + suffix + '; nothing detached')
            selected.append({'address': base + suffix, 'id': resource_id})
    azure_expiry_guard(state, {item['address'] for item in selected})
    print(json.dumps(selected, indent=2))
    if not args.detach:
        return
    os.umask(0o077)
    args.receipt_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    if args.receipt_dir.stat().st_mode & 0o077:
        raise RuntimeError('Receipt directory must be private (mode 0700)')
    # One immutable receipt per attempt preserves IDs across partial failures/retries.
    attempt = Path(tempfile.mkdtemp(prefix='detach-', dir=args.receipt_dir))
    save(attempt / 'state-before.json', state)
    save(attempt / 'retained.json', selected)
    save(attempt / 'manifest.json', manifest)
    save(attempt / 'source.json', {'directory': str(Path.cwd()), 'lineage': state.get('lineage'), 'serial': state.get('serial'), 'module': args.module_address})
    # Keep the exact inventory before touching cloud expiry, and stop before any state
    # removal until every selected table confirms DISABLED. A retry re-reads AWS.
    protect_dynamodb_ttl(state, selected)
    # Check for a concurrent writer between inventory and mutation. Terraform locks
    # the state mutation itself. Operators must serialize this entire teardown with
    # other Terraform runs, just as they do for a multi-command import procedure.
    current = json.loads(tf('state', 'pull'))
    if (current.get('lineage'), current.get('serial')) != (state.get('lineage'), state.get('serial')):
        raise RuntimeError('Terraform state changed; no resources detached. Retry after the other run finishes.')
    if selected:
        subprocess.run([CLI, 'state', 'rm', '-lock-timeout=60s', *[item['address'] for item in selected]], check=True)
    save(attempt / 'state-after.json', json.loads(tf('state', 'pull')))
    print('Recovery receipt: ' + str(attempt))
    print('Data is retained. Access has NOT yet been revoked. Run and review terraform plan -destroy, then apply that destroy plan. Do not run an ordinary apply after detaching.')


if __name__ == '__main__':
    CLI = os.environ.get('TF_CLI', 'terraform')
    if CLI not in ('terraform', 'tofu'):
        raise SystemExit('TF_CLI must be terraform or tofu')
    main()
