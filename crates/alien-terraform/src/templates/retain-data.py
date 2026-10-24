#!/usr/bin/env python3
"""Detach the generated setup's retained resources, keeping a recovery receipt."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def tf(*args):
    return subprocess.check_output([CLI, *args], text=True)


def save(path, value):
    # Never overwrite a recovery checkpoint, including after an interrupted detach.
    with path.open('x') as output:
        json.dump(value, output, indent=2)
        output.flush()
        os.fsync(output.fileno())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--detach', action='store_true', help='confirm detachment before teardown')
    parser.add_argument('--module-address', default='', help='exact module address when used as a child module')
    parser.add_argument('--receipt-dir', type=Path, required=True, help='private recovery directory outside the module')
    args = parser.parse_args()
    if os.name != "posix":
        raise RuntimeError("Use Linux, macOS, or a WSL Linux filesystem for private recovery receipts; native Windows is not supported")
    manifest = json.loads(Path(__file__).with_name('retained-data.json').read_text())
    prefix = args.module_address + '.' if args.module_address else ''
    addresses = {prefix + address for address in manifest}
    # A failed read must abort, never masquerade as an empty installation.
    state = json.loads(tf('state', 'pull'))
    if state.get('version') != 4 or not state.get('lineage') or not isinstance(state.get('serial'), int):
        raise RuntimeError('Unsupported or incomplete Terraform state; nothing detached')
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
            selected.append({'address': base + suffix, 'id': resource_id})
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
    save(attempt / 'source.json', {'directory': str(Path.cwd()), 'lineage': state.get('lineage'), 'serial': state.get('serial'), 'module': args.module_address})
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
