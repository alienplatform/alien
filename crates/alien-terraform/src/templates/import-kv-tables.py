#!/usr/bin/env python3
"""Move KV tables an earlier package declared as azurerm_storage_table to azapi_resource.

Run once after replacing an installed package with this one, before the next apply.
Each existing table is imported at its new address, then its old address is forgotten.
Nothing in Azure changes, and the table keeps its data. Without --apply, only the moves
are printed. Rerunning is safe: a finished move is skipped.
"""
import argparse
import json
import os
import subprocess

API_VERSION = '__TABLE_API_VERSION__'


def tf(*args):
    return subprocess.check_output([CLI, *args], text=True)


def address(resource, instance):
    base = '.'.join(filter(None, [resource.get('module'), resource['type'], resource['name']]))
    return base + ('[' + json.dumps(instance['index_key']) + ']' if 'index_key' in instance else '')


def moves(state, module):
    resources = [
        resource for resource in state.get('resources', [])
        if resource.get('mode') == 'managed' and resource.get('module', '') == module
    ]
    accounts = {}
    for resource in resources:
        if resource['type'] == 'azurerm_storage_account':
            for instance in resource.get('instances', []):
                attributes = instance['attributes']
                accounts[attributes['name'].lower()] = attributes['id']
    managed = {
        address(resource, instance)
        for resource in resources if resource['type'] == 'azapi_resource'
        for instance in resource.get('instances', [])
    }
    planned = []
    for resource in resources:
        if resource['type'] != 'azurerm_storage_table':
            continue
        target = dict(resource, type='azapi_resource')
        for instance in resource.get('instances', []):
            if instance.get('deposed'):
                raise RuntimeError('A deposed table is in state: ' + address(resource, instance) + '; nothing changed')
            attributes = instance['attributes']
            # AzureRM keeps storage_account_id null when the table uses storage_account_name.
            account = attributes.get('storage_account_id') or accounts.get((attributes.get('storage_account_name') or '').lower())
            if not account or not attributes.get('name'):
                raise RuntimeError('Cannot identify the storage account of ' + address(resource, instance) + '; nothing changed')
            planned.append({
                'from': address(resource, instance),
                'to': address(target, instance),
                'id': account + '/tableServices/default/tables/' + attributes['name'] + '?api-version=' + API_VERSION,
                'imported': address(target, instance) in managed,
            })
    return planned


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='import and forget the tables; without it, only print the moves')
    parser.add_argument('--module-address', default='', help='exact installed child module address')
    args = parser.parse_args()
    # A failed or empty read must abort, never masquerade as an installation without tables.
    pulled = tf('state', 'pull')
    if not pulled.strip():
        raise RuntimeError('No Terraform state found; run this where the installation was applied. Nothing changed')
    state = json.loads(pulled)
    if state.get('version') != 4 or not state.get('lineage'):
        raise RuntimeError('Unsupported or incomplete Terraform state; nothing changed')
    planned = moves(state, args.module_address)
    print(json.dumps(planned, indent=2))
    if not args.apply:
        return
    # Import before forgetting, so an interrupted run leaves every table tracked at least once.
    for move in planned:
        if not move['imported']:
            subprocess.run([CLI, 'import', '-input=false', move['to'], move['id']], check=True)
        subprocess.run([CLI, 'state', 'rm', move['from']], check=True)
    print('Moved ' + str(len(planned)) + ' table(s). Run and review a plan before applying.')


if __name__ == '__main__':
    CLI = os.environ.get('TF_CLI', 'terraform')
    if CLI not in ('terraform', 'tofu'):
        raise SystemExit('TF_CLI must be terraform or tofu')
    main()
