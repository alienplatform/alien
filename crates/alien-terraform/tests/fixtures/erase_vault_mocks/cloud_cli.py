#!/usr/bin/env python3
"""Stand-in for terraform, gcloud, aws and az in the erase helper tests.

The tool is the name this file is run under. It reads and rewrites $ERASE_MOCK_DIR/fixture.json
and appends each call to $ERASE_MOCK_DIR/calls.log.
"""
import json
import os
import sys
from pathlib import Path

directory = Path(os.environ['ERASE_MOCK_DIR'])
fixture_path = directory / 'fixture.json'
fixture = json.loads(fixture_path.read_text())
tool = Path(sys.argv[0]).name
raw_args = sys.argv[1:]
with (directory / 'calls.log').open('a') as log:
    log.write(tool + ' ' + ' '.join(raw_args) + '\n')
# `--flag=value` and `--flag value` are the same to the tools being imitated.
args = []
for argument in raw_args:
    if argument.startswith('--') and '=' in argument:
        args.extend(argument.split('=', 1))
    else:
        args.append(argument)


def flag(name):
    return args[args.index(name) + 1] if name in args else None


def save():
    fixture_path.write_text(json.dumps(fixture))


def fail(message):
    sys.stderr.write(message + '\n')
    sys.exit(1)


if tool == 'terraform':
    if args[:2] == ['state', 'pull']:
        print(json.dumps(fixture['state']))
    elif args[:3] == ['output', '-raw', 'deployment_resource_prefix'] and fixture.get('output_prefix'):
        print(fixture['output_prefix'], end='')
    else:
        fail('unsupported terraform call')
elif tool == 'gcloud':
    if args[:2] == ['secrets', 'list']:
        print(json.dumps(fixture['gcloud_secrets']))
    elif args[:3] == ['config', 'get-value', 'account']:
        print(fixture.get('gcloud_account', 'admin@example.com'))
    elif args[:2] == ['secrets', 'delete']:
        name = args[2]
        fixture['gcloud_secrets'] = [s for s in fixture['gcloud_secrets'] if not s['name'].endswith('/secrets/' + name)]
        save()
    else:
        fail('unsupported gcloud call')
elif tool == 'aws':
    if args[:2] == ['sts', 'get-caller-identity']:
        account = fixture.get('aws_account', '111122223333')
        print(json.dumps({'Account': account, 'Arn': 'arn:aws:iam::' + account + ':user/admin'}))
    elif args[:2] == ['ssm', 'describe-parameters']:
        prefix = flag('--parameter-filters').split('Values=', 1)[1]
        print(json.dumps({'Parameters': [{'Name': n, 'ARN': 'arn:aws:ssm:us-east-1:111122223333:parameter/' + n}
                                         for n in sorted(fixture['aws_tags']) if n.startswith(prefix)]}))
    elif args[:2] == ['ssm', 'list-tags-for-resource']:
        print(json.dumps({'TagList': fixture['aws_tags'][flag('--resource-id')]}))
    elif args[:2] == ['ssm', 'delete-parameter']:
        del fixture['aws_tags'][flag('--name')]
        save()
    else:
        fail('unsupported aws call')
elif tool == 'az':
    vault = flag('--vault-name')
    if args[:2] == ['account', 'show']:
        print(json.dumps({'id': fixture.get('az_subscription', 'sub-1'), 'user': {'name': 'admin@example.com'}}))
    elif args[:2] == ['keyvault', 'show']:
        settings = fixture['az_vaults'][flag('--name')]
        print(json.dumps({'purge': settings['purge'], 'days': settings['days']}))
    elif args[:3] == ['keyvault', 'secret', 'list']:
        # An entry is a name, or a dict with `name` and `tags`.
        print(json.dumps([item if isinstance(item, dict) else {'name': item, 'tags': None}
                          for item in fixture['az_secrets'][vault]]))
    elif args[:3] == ['keyvault', 'secret', 'delete']:
        fixture['az_secrets'][vault] = [item for item in fixture['az_secrets'][vault]
                                        if (item['name'] if isinstance(item, dict) else item) != flag('--name')]
        fixture.setdefault('az_soft_deleted', []).append(vault + '/' + flag('--name'))
        save()
    elif args[:3] == ['keyvault', 'secret', 'show-deleted']:
        # `az_show_deleted_misses`: how many calls report "not found" first; negative means always.
        misses = fixture.get('az_show_deleted_misses', 0)
        if misses != 0:
            if misses > 0:
                fixture['az_show_deleted_misses'] = misses - 1
                save()
            fail('SecretNotFound')
        print('{}')
    elif args[:3] == ['keyvault', 'secret', 'purge']:
        fixture['az_soft_deleted'].remove(vault + '/' + flag('--name'))
        save()
    else:
        fail('unsupported az call')
else:
    fail('unknown tool ' + tool)
