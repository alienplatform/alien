#!/usr/bin/env python3
"""Validate generated IAM policies in a disposable AWS test account.

Requires boto3 and AWS credentials. From the repository root:
  python3 scripts/validate-aws-boundary.py --account ACCOUNT --region us-west-2

Creates temporary IAM roles/policies, two S3 buckets, and one Lambda. All names
include a random run ID. Cleanup runs even on failure; cleanup errors fail the run.
The account argument must match STS before any resources are created.
"""

import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid
import zipfile

import boto3
from botocore.exceptions import ClientError


def document(statements):
    return {"Version": "2012-10-17", "Statement": statements}


def encode(value):
    return json.dumps(value, separators=(",", ":"))


def trust(principal):
    return encode(document([{"Effect": "Allow", "Principal": principal, "Action": "sts:AssumeRole"}]))


def eventually(operation, codes):
    for attempt in range(12):
        try:
            return operation()
        except ClientError as error:
            if error.response["Error"]["Code"] not in codes or attempt == 11:
                raise
            time.sleep(5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--account", required=True)
    parser.add_argument("--region", default="us-west-2")
    args = parser.parse_args()
    admin = boto3.Session(region_name=args.region)
    account = admin.client("sts").get_caller_identity()["Account"]
    if account != args.account:
        raise RuntimeError(f"Expected account {args.account}, got {account}")
    prefix = "pb" + uuid.uuid4().hex[:10]
    sibling = prefix + "b"
    print(f"Account {account}, region {args.region}, prefixes {prefix}, {sibling}", flush=True)
    with tempfile.TemporaryDirectory() as output:
        env = dict(os.environ, BOUNDARY_OUTPUT_DIR=output, BOUNDARY_PREFIX=prefix,
                   BOUNDARY_ACCOUNT=account, BOUNDARY_REGION=args.region)
        subprocess.run(["cargo", "test", "-p", "alien-preflights", "--test", "export_live_boundary",
                        "--", "--ignored"], env=env, check=True, stdout=subprocess.DEVNULL)
        boundary = json.loads(Path(output, "boundary.json").read_text())
        management = json.loads(Path(output, "management.json").read_text())
        tags = json.loads(Path(output, "tags.json").read_text())
    # Sids are optional labels; duplicate catalog labels cannot share a document.
    for statement in management["Statement"]:
        statement.pop("Sid", None)
    chunks = [[]]
    for statement in management["Statement"]:
        if len(encode(document(chunks[-1] + [statement]))) > 6000:
            chunks.append([])
        chunks[-1].append(statement)
    if len(chunks) > 10 or any(len(encode(document(chunk))) > 6144 for chunk in chunks):
        raise RuntimeError("Generated management policy exceeds test role managed-policy quotas")
    iam, s3, lamb = admin.client("iam"), admin.client("s3"), admin.client("lambda")
    boundary_arn = f"arn:aws:iam::{account}:policy/{prefix}-permissions-boundary"
    role_arn = lambda name: f"arn:aws:iam::{account}:role/{name}"
    lambda_trust = trust({"Service": "lambda.amazonaws.com"})
    admin_policy = encode(document([{"Effect": "Allow", "Action": "*", "Resource": "*"}]))
    policies, roles, buckets = [], [], []
    functions, results = [], []

    def create_role(client, name, **kwargs):
        result = client.create_role(RoleName=name, **kwargs)
        roles.append(name)
        return result

    def check(name, allowed, operation):
        try:
            operation()
            actual = True
        except ClientError as error:
            if error.response["Error"]["Code"] not in {"AccessDenied", "AccessDeniedException"}:
                raise
            actual = False
        results.append(actual == allowed)
        print(f"{'PASS' if actual == allowed else 'FAIL'} {name}: {'allowed' if actual else 'denied'}", flush=True)

    cleanup_errors = []

    def cleanup(operation):
        try:
            operation()
        except Exception as error:
            cleanup_errors.append(str(error))

    try:
        template = {"AWSTemplateFormatVersion": "2010-09-09", "Resources": {
            "Boundary": {"Type": "AWS::IAM::ManagedPolicy", "Properties": {
                "ManagedPolicyName": prefix + "-permissions-boundary", "PolicyDocument": boundary}}}}
        admin.client("cloudformation").validate_template(TemplateBody=encode(template))
        print(f"PASS CloudFormation validate-template; boundary {len(encode(boundary))} characters", flush=True)
        policies.append(iam.create_policy(PolicyName=prefix + "-permissions-boundary",
                                         PolicyDocument=encode(boundary))["Policy"]["Arn"])
        create_role(iam, prefix + "-management", AssumeRolePolicyDocument=trust({"AWS": f"arn:aws:iam::{account}:root"}))
        for index, chunk in enumerate(chunks):
            arn = iam.create_policy(PolicyName=f"{prefix}-management-{index}", PolicyDocument=encode(document(chunk)))["Policy"]["Arn"]
            policies.append(arn)
            iam.attach_role_policy(RoleName=prefix + "-management", PolicyArn=arn)
        for name in [prefix + "-legacy", sibling + "-admin"]:
            create_role(iam, name, AssumeRolePolicyDocument=trust({"AWS": f"arn:aws:iam::{account}:root"}))
            iam.put_role_policy(RoleName=name, PolicyName="admin", PolicyDocument=admin_policy)
        for name in [prefix, sibling]:
            bucket = f"{name}-data-{account}"
            location = {} if args.region == "us-east-1" else {"CreateBucketConfiguration": {"LocationConstraint": args.region}}
            s3.create_bucket(Bucket=bucket, **location)
            buckets.append(bucket)
            s3.put_object(Bucket=bucket, Key="marker", Body=b"boundary test")
        creds = eventually(lambda: admin.client("sts").assume_role(RoleArn=role_arn(prefix + "-management"), RoleSessionName="boundary-test")["Credentials"], {"AccessDenied"})
        manager = boto3.Session(aws_access_key_id=creds["AccessKeyId"], aws_secret_access_key=creds["SecretAccessKey"],
                               aws_session_token=creds["SessionToken"], region_name=args.region)
        miam = manager.client("iam")
        bounded = prefix + "-workload"
        # Establish positive permission propagation before interpreting denials.
        eventually(lambda: create_role(miam, bounded, AssumeRolePolicyDocument=lambda_trust,
                                       PermissionsBoundary=boundary_arn), {"AccessDenied"})
        print("PASS management creates bounded role", flush=True)
        check("create unbounded role", False, lambda: create_role(miam, prefix + "-unbounded", AssumeRolePolicyDocument=lambda_trust))
        check("write bounded role", True, lambda: miam.put_role_policy(RoleName=bounded, PolicyName="admin", PolicyDocument=admin_policy))
        check("attach managed administrator policy", False, lambda: miam.attach_role_policy(RoleName=bounded, PolicyArn="arn:aws:iam::aws:policy/AdministratorAccess"))
        check("write unbounded role", False, lambda: miam.put_role_policy(RoleName=prefix + "-legacy", PolicyName="extra", PolicyDocument=admin_policy))
        check("tag unbounded role", True, lambda: miam.tag_role(RoleName=prefix + "-legacy", Tags=[{"Key": "probe", "Value": "boundary"}]))
        check("delete unbounded inline policy", False, lambda: miam.delete_role_policy(RoleName=prefix + "-legacy", PolicyName="admin"))
        check("change unbounded trust", False, lambda: miam.update_assume_role_policy(RoleName=prefix + "-legacy", PolicyDocument=lambda_trust))
        check("remove boundary", False, lambda: miam.delete_role_permissions_boundary(RoleName=bounded))
        check("swap boundary", False, lambda: miam.put_role_permissions_boundary(RoleName=bounded, PermissionsBoundary="arn:aws:iam::aws:policy/AdministratorAccess"))
        check("rewrite boundary", False, lambda: miam.create_policy_version(PolicyArn=boundary_arn, PolicyDocument=admin_policy, SetAsDefault=True))
        check("create sibling role", False, lambda: create_role(miam, sibling + "-other", AssumeRolePolicyDocument=lambda_trust, PermissionsBoundary=boundary_arn))
        check("read sibling bucket", False, lambda: manager.client("s3").get_object(Bucket=buckets[1], Key="marker"))
        check("adopt legacy role", True, lambda: miam.put_role_permissions_boundary(RoleName=prefix + "-legacy", PermissionsBoundary=boundary_arn))
        check("write adopted role", True, lambda: miam.put_role_policy(RoleName=prefix + "-legacy", PolicyName="extra", PolicyDocument=admin_policy))
        check("delete adopted inline policy", True, lambda: miam.delete_role_policy(RoleName=prefix + "-legacy", PolicyName="extra"))
        check("change bounded trust", True, lambda: miam.update_assume_role_policy(RoleName=bounded, PolicyDocument=trust({"AWS": role_arn(prefix + "-management")})))
        check("chain into bounded role", False, lambda: manager.client("sts").assume_role(RoleArn=role_arn(bounded), RoleSessionName="chain"))
        miam.update_assume_role_policy(RoleName=bounded, PolicyDocument=lambda_trust)
        iam.attach_role_policy(RoleName=bounded, PolicyArn="arn:aws:iam::aws:policy/AdministratorAccess")
        code = '''import boto3
from botocore.exceptions import ClientError
def handler(event, context):
    results = {}
    for name, service, method, arguments in event:
        try:
            getattr(boto3.client(service), method)(**arguments)
            results[name] = "allowed"
        except ClientError as error:
            results[name] = error.response["Error"]["Code"]
    return results
'''
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("index.py", code)
        function = prefix + "-probe"
        eventually(lambda: manager.client("lambda").create_function(
            FunctionName=function, Runtime="python3.12", Handler="index.handler", Role=role_arn(bounded),
            Code={"ZipFile": buffer.getvalue()}, Timeout=30,
            Tags=tags), {"InvalidParameterValueException"})
        functions.append(function)
        lamb.get_waiter("function_active_v2").wait(FunctionName=function)
        probes = [
            ("own bucket", "s3", "get_object", {"Bucket": buckets[0], "Key": "marker"}),
            ("sibling bucket", "s3", "get_object", {"Bucket": buckets[1], "Key": "marker"}),
            ("EC2 discovery", "ec2", "describe_instances", {}),
            ("IAM read", "iam", "list_roles", {}),
            ("IAM write", "iam", "create_role", {"RoleName": prefix + "-escape", "AssumeRolePolicyDocument": lambda_trust}),
            ("bounded IAM write", "iam", "create_role", {"RoleName": prefix + "-escape2", "AssumeRolePolicyDocument": lambda_trust, "PermissionsBoundary": boundary_arn}),
            ("sibling assume", "sts", "assume_role", {"RoleArn": role_arn(sibling + "-admin"), "RoleSessionName": "probe"}),
        ]
        # Track these names too in case an unexpected successful write needs cleanup.
        roles.extend([prefix + "-escape", prefix + "-escape2"])
        response = lamb.invoke(FunctionName=function, Payload=encode(probes))
        payload = json.loads(response["Payload"].read())
        if response.get("FunctionError") or set(payload) != {probe[0] for probe in probes}:
            raise RuntimeError(f"Workload probe failed: {payload}")
        for name, result in payload.items():
            # Volume-backup runtime grants intentionally include account-wide EC2 metadata reads.
            ok = result == "allowed" if name in {"own bucket", "EC2 discovery"} else result in {"AccessDenied", "AccessDeniedException"}
            results.append(ok)
            print(f"{'PASS' if ok else 'FAIL'} workload {name}: {result}", flush=True)
    finally:
        for function in functions:
            cleanup(lambda: lamb.delete_function(FunctionName=function))
            def delete_logs():
                try:
                    admin.client("logs").delete_log_group(logGroupName="/aws/lambda/" + function)
                except ClientError as error:
                    if error.response["Error"]["Code"] != "ResourceNotFoundException":
                        raise
            cleanup(delete_logs)
        for role in reversed(roles):
            def delete_role():
                try:
                    for page in iam.get_paginator("list_role_policies").paginate(RoleName=role):
                        for policy in page["PolicyNames"]:
                            iam.delete_role_policy(RoleName=role, PolicyName=policy)
                    for page in iam.get_paginator("list_attached_role_policies").paginate(RoleName=role):
                        for policy in page["AttachedPolicies"]:
                            iam.detach_role_policy(RoleName=role, PolicyArn=policy["PolicyArn"])
                    iam.delete_role(RoleName=role)
                except ClientError as error:
                    if error.response["Error"]["Code"] != "NoSuchEntity":
                        raise
            cleanup(delete_role)
        for policy in reversed(policies):
            def delete_policy():
                for version in iam.list_policy_versions(PolicyArn=policy)["Versions"]:
                    if not version["IsDefaultVersion"]:
                        iam.delete_policy_version(PolicyArn=policy, VersionId=version["VersionId"])
                iam.delete_policy(PolicyArn=policy)
            cleanup(delete_policy)
        for bucket in buckets:
            cleanup(lambda: s3.delete_object(Bucket=bucket, Key="marker"))
            cleanup(lambda: s3.delete_bucket(Bucket=bucket))
        if cleanup_errors:
            raise RuntimeError(f"Cleanup failed for run {prefix}: {cleanup_errors}")
        print("Cleanup completed", flush=True)
    if not all(results):
        raise RuntimeError(f"{results.count(False)}/{len(results)} checks failed")
    print(f"PASS {len(results)} policy checks plus bounded creation and CloudFormation validation", flush=True)


if __name__ == "__main__":
    main()
