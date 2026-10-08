"""Create workload trust through Alien and verify actual AWS STS role chaining."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import uuid

import boto3
from botocore.exceptions import ClientError


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--expect-isolated-denied', action='store_true')
    args = parser.parse_args()
    region, account = os.environ['AWS_TARGET_REGION'], os.environ['AWS_TARGET_ACCOUNT_ID']
    session = boto3.Session(aws_access_key_id=os.environ['AWS_TARGET_ACCESS_KEY_ID'],
                            aws_secret_access_key=os.environ['AWS_TARGET_SECRET_ACCESS_KEY'],
                            aws_session_token=os.environ.get('AWS_TARGET_SESSION_TOKEN'), region_name=region)
    caller = session.client('sts').get_caller_identity()
    assert caller['Account'] == account
    prefix = 'e2e-1316-' + uuid.uuid4().hex[:8]
    iam = session.client('iam')
    workload = prefix + '-execution-sa'
    destination = f'arn:aws:iam::{account}:role/{workload}'
    created = []
    receipt = {'prefix': prefix, 'checks': [], 'cleanup': False}
    def check(message):
        receipt['checks'].append(message)
        print(message, flush=True)
    def assume(client, arn):
        deadline = time.monotonic() + 90
        while True:
            try:
                response = client.assume_role(RoleArn=arn, RoleSessionName='node-trust-proof')['Credentials']
                return boto3.client('sts', region_name=region,
                                    aws_access_key_id=response['AccessKeyId'],
                                    aws_secret_access_key=response['SecretAccessKey'],
                                    aws_session_token=response['SessionToken'])
            except ClientError as error:
                if error.response['Error']['Code'] != 'AccessDenied' or time.monotonic() >= deadline: raise
                time.sleep(2)
    def denied(client):
        try: client.assume_role(RoleArn=destination, RoleSessionName='denial-proof')
        except ClientError as error:
            assert error.response['Error']['Code'] == 'AccessDenied', error
        else: raise AssertionError('untrusted node unexpectedly assumed workload role')
    try:
        for suffix in ['compute-role', 'compute-isolation-v1-role', 'unrelated-role']:
            name = prefix + '-' + suffix
            trust = {'Version': '2012-10-17', 'Statement': [{
                'Effect': 'Allow', 'Principal': {'AWS': caller['Arn']}, 'Action': 'sts:AssumeRole'}]}
            iam.create_role(RoleName=name, AssumeRolePolicyDocument=json.dumps(trust),
                            Tags=[{'Key': 'alien.dev/test-run', 'Value': prefix}])
            created.append(name)
            policy = {'Version': '2012-10-17', 'Statement': [{
                'Effect': 'Allow', 'Action': 'sts:AssumeRole', 'Resource': destination}]}
            iam.put_role_policy(RoleName=name, PolicyName='test-chain', PolicyDocument=json.dumps(policy))
        env = dict(os.environ, ALIEN_TEST_TRUST_PREFIX=prefix)
        subprocess.run(['cargo', 'test', '-p', 'alien-infra', '--all-features', '--lib',
                        'live_create_service_account_for_compute_nodes', '--', '--ignored', '--nocapture'],
                       check=True, env=env)
        check('workload role created through the actual Alien controller')
        nodes = [assume(session.client('sts'), f'arn:aws:iam::{account}:role/{name}') for name in created]
        legacy = assume(nodes[0], destination)
        assert legacy.get_caller_identity()['Account'] == account
        check('legacy node successfully assumed workload role')
        if args.expect_isolated_denied:
            denied(nodes[1]); check('reproduced isolated node AccessDenied with old trust')
        else:
            isolated = assume(nodes[1], destination)
            assert isolated.get_caller_identity()['Account'] == account
            check('isolated node successfully assumed workload role')
        denied(nodes[2]); check('unrelated same-account node denied despite caller-side grant')
    finally:
        errors = []
        # Include the deterministic workload name even when create lost its response.
        for name in [workload] + created:
            try:
                for policy in iam.list_role_policies(RoleName=name)['PolicyNames']:
                    iam.delete_role_policy(RoleName=name, PolicyName=policy)
                iam.delete_role(RoleName=name)
            except ClientError as error:
                if error.response['Error']['Code'] != 'NoSuchEntity': errors.append(str(error))
        for name in [workload] + created:
            try: iam.get_role(RoleName=name)
            except ClientError as error:
                if error.response['Error']['Code'] != 'NoSuchEntity': errors.append(str(error))
            else: errors.append('role remains: ' + name)
        receipt['cleanup'] = not errors
        Path('/tmp/node-trust-live-receipt.json').write_text(json.dumps(receipt, indent=2))
        assert not errors, errors
        check('all task-owned IAM roles deleted and absence verified')

if __name__ == '__main__':
    main()
