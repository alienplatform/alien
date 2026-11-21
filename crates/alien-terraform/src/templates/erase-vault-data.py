#!/usr/bin/env python3
"""Erase the values an application wrote to this module's cloud secret store.

Run it after the deployment delete finishes and before `terraform destroy`, with your own cloud
credentials. It only looks at the vaults this module set up (see erase-vault-data.json), never at
a vault binding you supplied yourself.

Without --confirm it is a dry run: it lists three groups of values by exact name and deletes
nothing. A value is "marked" when the vault that created it recorded the vault prefix and id on
it, "unmarked" when it has the vault prefix in its name but no marker (written before markers
existed), and a "deployer secret" when it sits under the deployer secrets prefix.

On Azure every vault is its own Key Vault, so a secret is "marked" when its tags name that Key Vault
and vault id, "unmarked" when it has no marker, and a "deployer secret" when it sits in the deployer
secrets Key Vault. The helper deletes secrets, never a Key Vault.

Before it lists anything it checks that the signed-in CLI points at the account, region, project or
subscription this module was installed into, and refuses otherwise.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

PREFIX_MARKER_KEY = 'alien-vault-prefix'
ID_MARKER_KEY = 'alien-vault'
GROUPS = ('marked', 'unmarked', 'deployer')
AZURE_DEFAULT_RETENTION_DAYS = 90
# Key Vault deletes a secret asynchronously: a purge sent at once is refused until the deleted secret appears.
PURGE_POLL_SECONDS = float(os.environ.get('ERASE_PURGE_POLL_SECONDS', '2'))
PURGE_WAIT_SECONDS = float(os.environ.get('ERASE_PURGE_WAIT_SECONDS', '60'))


def run(argv):
    try:
        result = subprocess.run(argv, capture_output=True, text=True)
    except FileNotFoundError as error:
        raise RuntimeError(argv[0] + ' is required on PATH, signed in with your own credentials; nothing deleted') from error
    if result.returncode != 0:
        raise RuntimeError(' '.join(argv[:3]) + ' failed: ' + result.stderr.strip() + '; nothing deleted')
    return result.stdout


def tf(*args):
    return run([CLI, *args])


def save(path, value):
    # Publish only a complete, durable record, without replacing an earlier one.
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


def marker_value(raw):
    """The value a vault stores under a marker key; the same rule as the vault providers.

    A value that is 1 to 63 characters of [a-z0-9_-] is stored as is, anything else as `h-` and
    the first 40 hex digits of its SHA-256.
    """
    if re.fullmatch(r'[a-z0-9_-]{1,63}', raw):
        return raw
    return 'h-' + hashlib.sha256(raw.encode()).hexdigest()[:40]


def module_resources(state, module, kind, mode='managed'):
    for resource in state.get('resources', []):
        if (resource.get('mode') == mode and resource.get('type') == kind
                and resource.get('module', '') == module):
            yield resource


def attributes_of(state, module, kind, mode='managed'):
    for resource in module_resources(state, module, kind, mode):
        for instance in resource.get('instances', []):
            yield instance.get('attributes') or {}


def single_value(values, what):
    found = sorted({value for value in values if isinstance(value, str) and value})
    if not found:
        raise RuntimeError('The module state does not say ' + what + ', so the helper cannot check it against the signed-in '
                           'CLI. Use the same Terraform backend and workspace as the install; nothing deleted')
    if len(found) > 1:
        raise RuntimeError('The module state names more than one ' + what + ' (' + ', '.join(found) + '); nothing deleted')
    return found[0]


def refuse_mismatch(what, installed, current):
    if installed.lower() != current.lower():
        raise RuntimeError('This module was installed in ' + what + ' ' + installed + ', but the CLI points at ' + current
                           + '. Sign in to the install\'s ' + what + ' and pass the matching flags; nothing deleted')


def check_aws_scope(args, state, account):
    """The install's account and region must be the ones the CLI uses."""
    installed = list(attributes_of(state, args.module_address, 'aws_caller_identity', 'data'))
    accounts = [item.get('account_id') for item in installed]
    if not any(accounts):
        # Fall back to the account in the ARN of an IAM role or policy this module manages.
        for kind in ('aws_iam_role', 'aws_iam_policy'):
            for item in attributes_of(state, args.module_address, kind):
                parts = (item.get('arn') or '').split(':')
                if len(parts) > 5 and parts[0] == 'arn':
                    accounts.append(parts[4])
    refuse_mismatch('AWS account', single_value(accounts, 'its AWS account (no data.aws_caller_identity, IAM role or policy)'), account)
    regions = [item.get('region') or item.get('name')
               for item in attributes_of(state, args.module_address, 'aws_region', 'data')]
    refuse_mismatch('AWS region', single_value(regions, 'its AWS region (no data.aws_region)'), args.region)


def check_gcp_scope(args, state):
    """The install's project must be the one --project names."""
    projects = [item.get('project_id') for item in attributes_of(state, args.module_address, 'google_project', 'data')]
    if not any(projects):
        projects = [item.get('project') for item in attributes_of(state, args.module_address, 'google_service_account')]
    if not any(projects):
        projects = [(resource.get('instances') or [{}])[0].get('attributes', {}).get('project')
                    for resource in state.get('resources', [])
                    if resource.get('mode') == 'managed' and resource.get('module', '') == args.module_address
                    and str(resource.get('type', '')).startswith('google_')]
    refuse_mismatch('GCP project', single_value(projects, 'its GCP project (no data.google_project or resource project)'), args.project)


def check_azure_scope(args, state, subscription):
    """The install's Key Vaults must sit in the signed-in subscription."""
    found = []
    for item in attributes_of(state, args.module_address, 'azurerm_key_vault'):
        match = re.match(r'/subscriptions/([^/]+)/', item.get('id') or '', re.IGNORECASE)
        found.append(match.group(1).lower() if match else None)
    refuse_mismatch('Azure subscription', single_value(found, 'its Azure subscription (no Key Vault id)'), subscription or '')


def resolve_resource_prefix(args, state):
    """Return (prefix, generated). `generated` is true when it is the module's random prefix."""
    prefix = args.resource_prefix
    if not prefix:
        try:
            prefix = tf('output', '-raw', 'deployment_resource_prefix').strip()
        except RuntimeError as error:
            raise RuntimeError('Cannot read the module output deployment_resource_prefix (' + str(error)
                               + '). Pass --resource-prefix with the installed prefix') from error
    if not re.fullmatch(r'[a-z][a-z0-9-]{1,38}[a-z0-9]', prefix) or '--' in prefix:
        raise RuntimeError('Invalid resource prefix ' + json.dumps(prefix) + '; nothing deleted')
    randoms = list(module_resources(state, args.module_address, 'random_id'))
    randoms = [resource for resource in randoms if resource.get('name') == 'resource_prefix']
    if len(randoms) != 1 or len(randoms[0].get('instances', [])) != 1:
        raise RuntimeError('The module state has no generated resource prefix; check --module-address. Nothing deleted')
    generated = 'a' + randoms[0]['instances'][0]['attributes']['hex']
    return prefix, prefix == generated


def list_gcp(args, state, prefixes):
    project = args.project
    check_gcp_scope(args, state)
    entries = json.loads(run(['gcloud', 'secrets', 'list', '--project=' + project, '--format=json']))
    items = {}
    for entry in entries:
        resource = entry['name']
        name = resource.rsplit('/', 1)[-1]
        if any(name.startswith(prefix) for prefix in prefixes):
            items[name] = {'id': resource, 'labels': entry.get('labels') or {}}
    # With access-token authentication no account is configured; say so instead of recording nothing.
    account = run(['gcloud', 'config', 'get-value', 'account']).strip() or 'not set (access token)'
    return items, {'cloud': 'gcp', 'project': project, 'account': account}


def list_aws(args, state, prefixes):
    region = args.region
    identity = json.loads(run(['aws', 'sts', 'get-caller-identity', '--output=json', '--no-cli-pager']))
    check_aws_scope(args, state, identity['Account'])
    items = {}
    for prefix in sorted(prefixes):
        listing = json.loads(run(['aws', 'ssm', 'describe-parameters', '--region=' + region,
                                  '--parameter-filters=Key=Name,Option=BeginsWith,Values=' + prefix,
                                  '--output=json', '--no-cli-pager']))
        for parameter in listing.get('Parameters', []):
            name = parameter['Name']
            if name.startswith(prefix):
                arn = parameter.get('ARN') or 'arn:aws:ssm:{}:{}:parameter/{}'.format(region, identity['Account'], name)
                items[name] = {'id': arn, 'labels': None}
    return items, {'cloud': 'aws', 'region': region, 'account': identity['Account'], 'caller': identity.get('Arn')}


def load_aws_tags(args, items, names):
    for name in names:
        tags = json.loads(run(['aws', 'ssm', 'list-tags-for-resource', '--region=' + args.region,
                               '--resource-type=Parameter', '--resource-id=' + name,
                               '--output=json', '--no-cli-pager']))
        items[name]['labels'] = {tag['Key']: tag['Value'] for tag in tags.get('TagList', [])}


def classify(resource_prefix, config, items):
    """Sort values into the three groups; everything else is left alone, with the reason."""
    deployer_prefix = resource_prefix + '-' + config['deployer_secrets_vault_id'] + '-'
    prefixes = {vault_id: resource_prefix + '-' + vault_id for vault_id in config['vault_ids']}
    groups = {group: [] for group in GROUPS}
    left_alone = []
    for name in sorted(items):
        item = items[name]
        entry = {'name': name, 'id': item['id']}
        if name.startswith(deployer_prefix):
            groups['deployer'].append(entry)
            continue
        labels = item['labels'] or {}
        # Both markers must match one vault of this module exactly, and the name must carry its prefix.
        if any(name.startswith(prefix + '-')
               and labels.get(PREFIX_MARKER_KEY) == marker_value(prefix)
               and labels.get(ID_MARKER_KEY) == marker_value(vault_id)
               for vault_id, prefix in prefixes.items()):
            groups['marked'].append(entry)
            continue
        if PREFIX_MARKER_KEY in labels or ID_MARKER_KEY in labels:
            left_alone.append({**entry, 'reason': 'marked for a vault this module did not set up'})
            continue
        owners = [vault_id for vault_id, prefix in prefixes.items() if name.startswith(prefix + '-')]
        if not owners:
            continue
        # A name that also starts with another vault's prefix (`secrets` and `secrets-x`) could
        # belong to either vault, so it is never offered as unmarked.
        if any(name.startswith(other + '-') for vault_id in owners
               for other_id, other in prefixes.items() if other_id != vault_id):
            left_alone.append({**entry, 'reason': 'its name also starts with another vault prefix of this module'})
            continue
        groups['unmarked'].append(entry)
    return groups, left_alone


def preview_hash(scope, resource_prefix, groups):
    canonical = json.dumps({'scope': scope, 'resource_prefix': resource_prefix,
                            'groups': {group: [[entry['name'], entry['id']] for entry in groups[group]]
                                       for group in GROUPS}}, sort_keys=True)
    return hashlib.sha256(canonical.encode()).hexdigest()


def custom_prefix_warning(prefix):
    return ('WARNING: the resource prefix "' + prefix + '" came from var.resource_prefix, not the random one this module '
            'generates. Another install that uses the same prefix writes values with the same names, so the '
            '"marked" and "unmarked" groups below may include values that are not this install\'s: both kinds of '
            'value carry the same vault prefix and vault id. Check each name before using --confirm.')


OPERATOR_WARNING = ('WARNING: this module\'s Terraform state still has the Operator Helm release, so the chart is still installed and '
                    'values may still change. The order in README.md is to delete the deployment and wait until its status is '
                    '`teardown-required`, `retention-required` or `deleted` first, then run this helper. The check at --confirm '
                    'still refuses if the set changed since the preview.')


def print_group(title, entries, how):
    print('{} ({}): {}'.format(title, len(entries), how))
    for entry in entries:
        print('  - {}  [{}]'.format(entry['name'], entry['id']))


def list_azure(args, state, config):
    """Group the secrets of the Key Vaults this module created; a vault not in this module is never read."""
    addresses = {address: vault_id for vault_id, address in config['azure_key_vaults'].items()}
    account = json.loads(run(['az', 'account', 'show', '--output=json']))
    scope = {'cloud': 'azure', 'subscription': account.get('id'), 'account': (account.get('user') or {}).get('name')}
    check_azure_scope(args, state, account.get('id'))
    groups = {group: [] for group in GROUPS}
    left_alone = []
    vaults = {}
    for resource in module_resources(state, args.module_address, 'azurerm_key_vault'):
        vault_id = addresses.get(resource.get('type') + '.' + resource.get('name'))
        for instance in resource.get('instances', []):
            attributes = instance['attributes']
            vault = attributes['name']
            if vault_id is None:
                left_alone.append({'name': vault, 'id': attributes.get('id'), 'reason': 'a Key Vault of this module that is not one of its vaults'})
                continue
            settings = json.loads(run(['az', 'keyvault', 'show', '--name=' + vault, '--query=' +
                                       '{purge: properties.enablePurgeProtection, days: properties.softDeleteRetentionInDays}',
                                       '--output=json']))
            vaults[vault] = {'purge_protection': bool(settings.get('purge')),
                             'retention_days': settings.get('days') or AZURE_DEFAULT_RETENTION_DAYS}
            secrets = json.loads(run(['az', 'keyvault', 'secret', 'list', '--vault-name=' + vault,
                                      '--query=[].{name: name, tags: tags}', '--output=json']))
            for secret in sorted(secrets, key=lambda item: item['name']):
                name = secret['name']
                entry = {'name': name, 'id': 'https://{}.vault.azure.net/secrets/{}'.format(vault, name), 'vault': vault}
                if vault_id == config['deployer_secrets_vault_id']:
                    groups['deployer'].append(entry)
                    continue
                tags = secret.get('tags') or {}
                if tags.get(PREFIX_MARKER_KEY) == marker_value(vault) and tags.get(ID_MARKER_KEY) == marker_value(vault_id):
                    groups['marked'].append(entry)
                elif PREFIX_MARKER_KEY in tags or ID_MARKER_KEY in tags:
                    left_alone.append({**entry, 'reason': 'marked for a vault this module did not set up'})
                else:
                    groups['unmarked'].append(entry)
    return groups, left_alone, scope, vaults


def azure_after_delete(vault, settings, deleted_at):
    """What happens to the deleted secrets of one vault: purged now, or recoverable until a date."""
    if not settings['purge_protection']:
        return {'purged': True, 'purge_blocked_until': None}
    until = (deleted_at + timedelta(days=settings['retention_days'])).date().isoformat()
    return {'purged': False, 'purge_blocked_until': until}


def wait_until_deleted(vault, name):
    """Wait until Key Vault lists the deleted secret, so that a purge is accepted."""
    deadline = time.monotonic() + PURGE_WAIT_SECONDS
    while True:
        try:
            run(['az', 'keyvault', 'secret', 'show-deleted', '--vault-name=' + vault, '--name=' + name])
            return True
        except RuntimeError:
            if time.monotonic() >= deadline:
                return False
            time.sleep(PURGE_POLL_SECONDS)


def azure_vault_notes(vaults):
    now = datetime.now(timezone.utc)
    for vault, settings in sorted(vaults.items()):
        if settings['purge_protection']:
            print('Key Vault {}: purge protection is on. Deleted values stay recoverable in its soft-delete state until about {} '
                  '({} days), because Key Vault purge protection does not allow removing them sooner. They are not purged, and '
                  'until then the same secret name cannot be created again unless you recover the deleted secret.'
                  .format(vault, azure_after_delete(vault, settings, now)['purge_blocked_until'], settings['retention_days']))
        else:
            print('Key Vault {}: purge protection is off. Each deleted value is purged right after it is deleted.'.format(vault))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--confirm', action='store_true', help='delete the marked values listed by the preview')
    parser.add_argument('--include-unmarked', action='store_true', help='with --confirm, also delete the unmarked values')
    parser.add_argument('--include-deployer-secrets', action='store_true', help='with --confirm, also delete the deployer secrets')
    parser.add_argument('--shared-prefix-reviewed', action='store_true',
                        help='with --confirm and a resource_prefix you set yourself, confirm that no other install uses that prefix')
    parser.add_argument('--module-address', default='', help='exact installed child module address')
    parser.add_argument('--resource-prefix', default='', help='installed resource prefix; default: the deployment_resource_prefix output')
    parser.add_argument('--project', default='', help='GCP project that holds the secrets (GKE modules)')
    parser.add_argument('--region', default='', help='AWS region that holds the parameters (EKS modules)')
    parser.add_argument('--receipt-dir', type=Path, required=True, help='private directory for previews and receipts, outside the module')
    args = parser.parse_args()
    if os.name != 'posix':
        raise RuntimeError('Use Linux, macOS, or a WSL Linux filesystem for private receipts; native Windows is not supported')
    config = json.loads(Path(__file__).with_name('erase-vault-data.json').read_text())
    if config.get('version') != 1 or config.get('cloud') not in ('gcp', 'aws', 'azure'):
        raise RuntimeError('Invalid erase-vault-data.json; nothing deleted')
    # A failed read must abort, never masquerade as an empty installation.
    state = json.loads(tf('state', 'pull'))
    if state.get('version') != 4 or not state.get('lineage') or not isinstance(state.get('serial'), int):
        raise RuntimeError('Unsupported or incomplete Terraform state; nothing deleted')
    azure = config['cloud'] == 'azure'
    if config['cloud'] == 'gcp' and not args.project:
        raise RuntimeError('--project is required for a GKE module; nothing deleted')
    if config['cloud'] == 'aws' and not args.region:
        raise RuntimeError('--region is required for an EKS module; nothing deleted')

    operator_chart_installed = any(True for _ in module_resources(state, args.module_address, 'helm_release'))
    vaults = {}
    if azure:
        # The Key Vaults come from this module's state, so no resource prefix or name matching is involved.
        resource_prefix, generated = '', True
        groups, left_alone, scope, vaults = list_azure(args, state, config)
    else:
        resource_prefix, generated = resolve_resource_prefix(args, state)
        prefixes = {resource_prefix + '-' + vault_id + '-' for vault_id in config['vault_ids']}
        prefixes.add(resource_prefix + '-' + config['deployer_secrets_vault_id'] + '-')
        if config['cloud'] == 'gcp':
            items, scope = list_gcp(args, state, prefixes)
        else:
            items, scope = list_aws(args, state, prefixes)
            # Tags are only needed for values that are not deployer secrets.
            deployer_prefix = resource_prefix + '-' + config['deployer_secrets_vault_id'] + '-'
            load_aws_tags(args, items, [name for name in sorted(items) if not name.startswith(deployer_prefix)])
        groups, left_alone = classify(resource_prefix, config, items)
    digest = preview_hash(scope, resource_prefix, groups)

    os.umask(0o077)
    args.receipt_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
    if args.receipt_dir.stat().st_mode & 0o077:
        raise RuntimeError('Receipt directory must be private (mode 0700)')
    preview_path = args.receipt_dir / ('preview-' + digest + '.json')

    if not args.confirm:
        if not preview_path.exists():
            save(preview_path, {'hash': digest, 'time': datetime.now(timezone.utc).isoformat(),
                                'scope': scope, 'resource_prefix': resource_prefix, 'groups': groups,
                                'left_alone': left_alone})
        print('Dry run: nothing was deleted. Scope: ' + json.dumps(scope) + ', resource prefix ' + resource_prefix + '.')
        if not generated:
            print(custom_prefix_warning(resource_prefix))
        if operator_chart_installed:
            print(OPERATOR_WARNING)
        print_group('marked', groups['marked'], 'created by this module\'s vaults; deleted by --confirm')
        if azure:
            print_group('unmarked', groups['unmarked'],
                        'sit in this module\'s Key Vaults and carry no marker; deleted only by --confirm --include-unmarked')
            print_group('deployer secrets', groups['deployer'],
                        'sit in this module\'s deployer secrets Key Vault; deleted only by --confirm --include-deployer-secrets')
            azure_vault_notes(vaults)
            print('The Key Vaults themselves are retained setup data: this helper never deletes or purges a Key Vault. '
                  'See the "Retain Azure setup data during uninstall" section of README.md.')
        else:
            print_group('unmarked', groups['unmarked'],
                        'carry no marker and sit under this module\'s vault prefixes; deleted only by --confirm --include-unmarked')
            print_group('deployer secrets', groups['deployer'],
                        'sit under ' + resource_prefix + '-' + config['deployer_secrets_vault_id'] + '-; deleted only by --confirm --include-deployer-secrets')
        if left_alone:
            print('left alone ({}): never deleted by this helper'.format(len(left_alone)))
            for entry in left_alone:
                print('  - {}  [{}]  {}'.format(entry['name'], entry['id'], entry['reason']))
        print('Preview hash: ' + digest + ' (saved in ' + str(args.receipt_dir) + ')')
        print('Review the names, then rerun with --confirm (and the --include flags you want) to delete exactly these values.')
        return

    if not generated and not args.shared_prefix_reviewed:
        raise RuntimeError('The resource prefix "' + resource_prefix + '" was set with var.resource_prefix, so another install may use '
                           'it and write values with the same names and markers. Confirm that no other install uses it and rerun '
                           'with --shared-prefix-reviewed; nothing deleted')
    # The set must be the one that was previewed: a value written or removed since then stops here.
    if not preview_path.exists():
        raise RuntimeError('The values changed since the preview, or no preview was run for this scope. Run the preview again (without --confirm) and review it; nothing deleted')
    if not generated:
        print(custom_prefix_warning(resource_prefix))
    if operator_chart_installed:
        print(OPERATOR_WARNING)
    selected = [(group, entry) for group in GROUPS
                if group == 'marked'
                or (group == 'unmarked' and args.include_unmarked)
                or (group == 'deployer' and args.include_deployer_secrets)
                for entry in groups[group]]
    flags = {'confirm': True, 'include_unmarked': args.include_unmarked,
             'include_deployer_secrets': args.include_deployer_secrets,
             'shared_prefix_reviewed': args.shared_prefix_reviewed}
    attempt = Path(tempfile.mkdtemp(prefix='erase-', dir=args.receipt_dir))
    started = datetime.now(timezone.utc).isoformat()
    base = {'time': started, 'flags': flags, 'scope': scope, 'resource_prefix': resource_prefix,
            'preview_hash': digest, 'module': args.module_address}
    save(attempt / 'intent.json', {**base, 'planned': [{'name': entry['name'], 'id': entry['id'], 'group': group}
                                                      for group, entry in selected]})
    deleted = []
    failure = None
    for group, entry in selected:
        try:
            if config['cloud'] == 'gcp':
                run(['gcloud', 'secrets', 'delete', entry['name'], '--project=' + args.project, '--quiet'])
            elif config['cloud'] == 'aws':
                run(['aws', 'ssm', 'delete-parameter', '--region=' + args.region, '--name=' + entry['name'],
                     '--no-cli-pager'])
            else:
                run(['az', 'keyvault', 'secret', 'delete', '--vault-name=' + entry['vault'], '--name=' + entry['name']])
        except RuntimeError as error:
            failure = str(error)
            break
        record = {'name': entry['name'], 'id': entry['id'], 'group': group}
        if azure:
            record['vault'] = entry['vault']
            record.update(azure_after_delete(entry['vault'], vaults[entry['vault']], datetime.now(timezone.utc)))
            if record['purged']:
                try:
                    if not wait_until_deleted(entry['vault'], entry['name']):
                        raise RuntimeError('still being deleted; purge it later with az keyvault secret purge '
                                           '--vault-name {} --name {}'.format(entry['vault'], entry['name']))
                    run(['az', 'keyvault', 'secret', 'purge', '--vault-name=' + entry['vault'], '--name=' + entry['name']])
                except RuntimeError as error:
                    record['purged'] = False
                    record['error'] = str(error)
                    deleted.append(record)
                    failure = str(error)
                    break
        deleted.append(record)
    save(attempt / 'receipt.json', {**base, 'deleted': deleted, 'error': failure,
                                    **({'key_vaults': vaults} if azure else {})})
    print('Receipt: ' + str(attempt / 'receipt.json'))
    if failure:
        raise RuntimeError(failure.replace('nothing deleted', '{} deleted before this; see the receipt'.format(len(deleted))))
    print('Deleted {} value(s).'.format(len(deleted)))
    if azure:
        azure_vault_notes({vault: vaults[vault] for vault in sorted({record['vault'] for record in deleted})})


if __name__ == '__main__':
    CLI = os.environ.get('TF_CLI', 'terraform')
    if CLI not in ('terraform', 'tofu'):
        raise SystemExit('TF_CLI must be terraform or tofu')
    try:
        main()
    except RuntimeError as error:
        message = str(error).strip().rstrip('.')
        if '--confirm' in sys.argv[1:]:
            message += '. Run the preview again before --confirm'
        print('error: ' + message, file=sys.stderr)
        raise SystemExit(1)
