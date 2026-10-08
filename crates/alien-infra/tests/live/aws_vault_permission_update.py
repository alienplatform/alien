"""Live permission-update proof; creates and deletes only per-run AWS fixtures."""
import json
import os
from pathlib import Path
import subprocess
import time
import uuid

import boto3
from botocore.exceptions import ClientError


def main():
    region = os.environ['AWS_TARGET_REGION']
    expected = os.environ['AWS_TARGET_ACCOUNT_ID']
    session = boto3.Session(aws_access_key_id=os.environ['AWS_TARGET_ACCESS_KEY_ID'],
                            aws_secret_access_key=os.environ['AWS_TARGET_SECRET_ACCESS_KEY'],
                            aws_session_token=os.environ.get('AWS_TARGET_SESSION_TOKEN'),
                            region_name=region)
    caller = session.client('sts').get_caller_identity()
    assert caller['Account'] == expected
    prefix = 'e2e-1314-' + uuid.uuid4().hex[:8]
    iam, ssm = session.client('iam'), session.client('ssm')
    roles, parameters = [], []
    receipt = {'prefix': prefix, 'checks': [], 'cleanup': False}
    def check(name):
        receipt['checks'].append(name)
        print(name, flush=True)
    def assume(role):
        deadline = time.monotonic() + 90
        while True:
            try:
                value = session.client('sts').assume_role(RoleArn=role, RoleSessionName='vault-update-proof')['Credentials']
                return boto3.client('ssm', region_name=region,
                                    aws_access_key_id=value['AccessKeyId'],
                                    aws_secret_access_key=value['SecretAccessKey'],
                                    aws_session_token=value['SessionToken'])
            except ClientError as error:
                if error.response['Error']['Code'] != 'AccessDenied' or time.monotonic() >= deadline:
                    raise
                time.sleep(2)
    def denied(client, name):
        try:
            client.get_parameter(Name=name, WithDecryption=True)
        except ClientError as error:
            assert error.response['Error']['Code'] in ('AccessDeniedException', 'AccessDenied'), error
        else:
            raise AssertionError('ungranted read unexpectedly succeeded')
    try:
        for suffix in ['consumer-sa', 'unrelated-sa']:
            name = prefix + '-' + suffix
            policy = {'Version': '2012-10-17', 'Statement': [{
                'Effect': 'Allow', 'Principal': {'AWS': caller['Arn']}, 'Action': 'sts:AssumeRole'}]}
            iam.create_role(RoleName=name, AssumeRolePolicyDocument=json.dumps(policy),
                            Tags=[{'Key': 'alien.dev/test-run', 'Value': prefix}])
            roles.append(name)
        for suffix in ['secrets-canary', 'other-canary']:
            name = prefix + '-' + suffix
            ssm.put_parameter(Name=name, Value='synthetic-vault-update-canary', Type='SecureString',
                              Tags=[{'Key': 'alien.dev/test-run', 'Value': prefix}])
            parameters.append(name)
        consumer = assume(f'arn:aws:iam::{expected}:role/{roles[0]}')
        unrelated = assume(f'arn:aws:iam::{expected}:role/{roles[1]}')
        denied(consumer, parameters[0]); check('new consumer denied before setup update')
        env = dict(os.environ, ALIEN_TEST_VAULT_PREFIX=prefix)
        subprocess.run(['cargo', 'test', '-p', 'alien-infra', '--all-features', '--lib',
                        'live_permission_only_update_reconciles_existing_vault', '--', '--ignored', '--nocapture'],
                       check=True, env=env)
        check('real executor reconciled existing vault and converged')
        consumer = assume(f'arn:aws:iam::{expected}:role/{roles[0]}')
        deadline = time.monotonic() + 90
        while True:
            try:
                response = consumer.get_parameter(Name=parameters[0], WithDecryption=True)
                assert response['Parameter']['Value'] == 'synthetic-vault-update-canary'
                break
            except ClientError as error:
                if error.response['Error']['Code'] not in ('AccessDeniedException', 'AccessDenied') or time.monotonic() >= deadline:
                    raise
                time.sleep(2)
        check('new consumer read and decrypted real SSM canary')
        denied(consumer, parameters[1]); check('consumer denied unrelated vault namespace')
        denied(unrelated, parameters[0]); check('unrelated role denied updated vault')
    finally:
        errors = []
        for name in parameters:
            try: ssm.delete_parameter(Name=name)
            except Exception as error: errors.append(str(error))
        for name in roles:
            try:
                for policy in iam.list_role_policies(RoleName=name)['PolicyNames']:
                    iam.delete_role_policy(RoleName=name, PolicyName=policy)
                iam.delete_role(RoleName=name)
            except Exception as error: errors.append(str(error))
        for name in roles:
            try: iam.get_role(RoleName=name)
            except ClientError as error:
                if error.response['Error']['Code'] != 'NoSuchEntity': errors.append(str(error))
            else: errors.append('role remained after deletion: ' + name)
        for name in parameters:
            try: ssm.get_parameter(Name=name)
            except ClientError as error:
                if error.response['Error']['Code'] != 'ParameterNotFound': errors.append(str(error))
            else: errors.append('parameter remained after deletion: ' + name)
        receipt['cleanup'] = not errors
        Path('/tmp/vault-update-live-receipt.json').write_text(json.dumps(receipt, indent=2))
        assert not errors, errors
        check('all task-owned roles and parameters deleted and absence verified')

if __name__ == '__main__':
    main()
