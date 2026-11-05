#!/usr/bin/env python3
"""Check on real AWS that a rendered setup locks what it pins away from the management role.

Renders `app/` as an automatic-setup CloudFormation stack, deploys it into the default VPC of a
disposable test account, assumes the stack's management role and tries what a compromised
control plane would try:

  * boot an image whose owner is not the recorded machine image publisher;
  * pass a workload (`-sa`) role to EC2 instead of a compute role;
  * write the anchor setup wrote, and the deployer's inputs.

It tries each twice: with the management policies alone, and again after attaching a policy
that allows all of it, as an extended profile or a customer policy could. A locked stack must
refuse every attempt both times, and still allow the publisher's images and the compute roles.

With --baseline-alien (an `alien` built from a commit without the locks) or
--baseline-template (a template it rendered), the same attempts run against that stack and
must succeed, so the check cannot pass on attempts AWS refuses for an unrelated reason.

Everything created is named with --prefix, tagged with --tag and with a tag unique to the run,
and deleted before exit. The run then lists what still carries its run tag or the prefix and
fails if anything does. Use a prefix no other run in the account uses.
Calls are dry runs or free; a run costs nothing beyond a few minutes of CloudFormation.

  python3 scripts/aws-setup-locks/check.py --alien target/debug/alien \\
      --baseline-alien /tmp/alien-baseline --prefix locks-check --tag purpose=locks-check \\
      --evidence /tmp/locks-evidence

Requires boto3 and administrator credentials for the test account in the environment.
"""

import argparse
import base64
import json
import re
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import boto3
from botocore.exceptions import ClientError, WaiterError

HERE = Path(__file__).resolve().parent
# ec2:Owner is an image owner's alias when it has one ("amazon" also covers some vendors'
# images, such as Canonical's and Debian's), and its account ID otherwise. The publisher stands
# in for a regular account without an alias: by default the AlmaLinux project's.
DEFAULT_PUBLISHER = "764336703387"
OTHER_IMAGE = "/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64"


class Run:
    def __init__(self, args):
        self.args = args
        self.session = boto3.session.Session(region_name=args.region)
        self.cfn = self.session.client("cloudformation")
        self.iam = self.session.client("iam")
        self.ec2 = self.session.client("ec2")
        self.ssm = self.session.client("ssm")
        self.sts = self.session.client("sts")
        self.account = self.sts.get_caller_identity()["Account"]
        self.s3 = self.session.client("s3")
        key, value = args.tag.split("=", 1)
        self.tag = {"Key": key, "Value": value}
        # Other runs in the account may carry the same tag; the audit looks for this one.
        self.run_tag = {"Key": "check-run", "Value": uuid.uuid4().hex[:12]}
        self.bucket = None
        self.evidence = Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.cleanups = []
        self.results = []

    # Evidence

    def record(self, name, payload):
        path = self.evidence / f"{name}.json"
        path.write_text(json.dumps(payload, indent=2, default=str) + "\n")
        return path

    def expect(self, variant, case, expected, actual, detail):
        ok = actual == expected
        self.results.append(
            {"variant": variant, "case": case, "expected": expected, "actual": actual, "ok": ok}
        )
        self.record(f"{variant}-{case}", {"expected": expected, "actual": actual, "detail": detail})
        print(f"{'ok  ' if ok else 'FAIL'} {variant:8} {case:44} expected {expected:24} got {actual}")

    # Setup

    def tags(self, extra=()):
        return [self.tag, self.run_tag, *extra]

    def default_network(self):
        vpcs = self.ec2.describe_vpcs(Filters=[{"Name": "is-default", "Values": ["true"]}])["Vpcs"]
        if len(vpcs) != 1:
            raise SystemExit("the region needs exactly one default VPC")
        vpc = vpcs[0]["VpcId"]
        subnets = self.ec2.describe_subnets(
            Filters=[
                {"Name": "vpc-id", "Values": [vpc]},
                {"Name": "default-for-az", "Values": ["true"]},
            ]
        )["Subnets"]
        if not subnets:
            raise SystemExit("the default VPC has no default subnet")
        return vpc, sorted(subnets, key=lambda subnet: subnet["AvailabilityZone"])[0]["SubnetId"]

    def caller_role(self):
        """The role the stacks trust as their managing role: this account may assume it."""
        name = f"{self.args.prefix}-caller"
        trust = {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": {"AWS": f"arn:aws:iam::{self.account}:root"},
                    "Action": "sts:AssumeRole",
                }
            ],
        }
        arn = self.iam.create_role(
            RoleName=name, AssumeRolePolicyDocument=json.dumps(trust), Tags=self.tags()
        )["Role"]["Arn"]
        self.cleanups.append(lambda: self.iam.delete_role(RoleName=name))
        policy = {
            "Version": "2012-10-17",
            "Statement": [{"Effect": "Allow", "Action": "sts:AssumeRole", "Resource": "*"}],
        }
        self.iam.put_role_policy(
            RoleName=name, PolicyName="assume", PolicyDocument=json.dumps(policy)
        )
        self.cleanups.append(lambda: self.iam.delete_role_policy(RoleName=name, PolicyName="assume"))
        return arn

    def settings(self, directory, publisher):
        """Automatic setup in the default VPC, protected, with `publisher` owning the images."""
        der = subprocess.run(
            "openssl ecparam -name prime256v1 -genkey -noout | openssl ec -pubout -outform DER",
            shell=True,
            check=True,
            capture_output=True,
        ).stdout
        settings = {
            "setupManagement": "automatic",
            "network": {"type": "use-default"},
            "machineImagePublisher": {"awsAccountId": publisher},
            "releaseTrust": {
                "project": "prj_setup_locks",
                "deployment": "dep_setup_locks",
                "signing": "SHA256:" + "0123456789abcdef" * 4,
                "components": "ecdsa-p256:" + base64.b64encode(der).decode(),
                "sessionApproval": "developer",
            },
        }
        path = directory / "settings.json"
        path.write_text(json.dumps(settings, indent=2))
        return path

    def render(self, alien, settings, output):
        subprocess.run(
            [
                str(alien), "render", "--format", "cloudformation", "--stack", str(HERE / "app"),
                "--stack-settings", str(settings), "--registration-mode", "outputs", "--json",
                "--output", str(output),
            ],
            check=True,
        )
        return output / "template.json"

    def template_bucket(self):
        """Templates over 51,200 bytes deploy from S3."""
        if self.bucket is None:
            name = f"{self.args.prefix}-templates-{self.run_tag['Value']}"
            request = {"Bucket": name}
            if self.args.region != "us-east-1":
                request["CreateBucketConfiguration"] = {"LocationConstraint": self.args.region}
            self.s3.create_bucket(**request)
            self.cleanups.append(lambda: self.s3.delete_bucket(Bucket=name))
            self.s3.put_bucket_tagging(Bucket=name, Tagging={"TagSet": self.tags()})
            self.bucket = name
        return self.bucket

    def deploy(self, stack_name, template, managing_role):
        bucket = self.template_bucket()
        key = f"{stack_name}.json"
        self.s3.put_object(Bucket=bucket, Key=key, Body=template.read_bytes())
        self.cleanups.append(lambda: self.s3.delete_object(Bucket=bucket, Key=key))
        url = f"https://{bucket}.s3.{self.args.region}.amazonaws.com/{key}"
        self.cfn.create_stack(
            StackName=stack_name,
            TemplateURL=url,
            Parameters=[{"ParameterKey": "ManagingRoleArn", "ParameterValue": managing_role}],
            Capabilities=["CAPABILITY_NAMED_IAM"],
            Tags=self.tags(),
        )
        self.cleanups.append(lambda: self.delete_stack(stack_name))
        try:
            self.cfn.get_waiter("stack_create_complete").wait(
                StackName=stack_name, WaiterConfig={"Delay": 15, "MaxAttempts": 80}
            )
        except WaiterError:
            events = self.cfn.describe_stack_events(StackName=stack_name)["StackEvents"]
            self.record(f"{stack_name}-events", events[:30])
            raise

    def delete_stack(self, stack_name):
        self.cfn.delete_stack(StackName=stack_name)
        self.cfn.get_waiter("stack_delete_complete").wait(
            StackName=stack_name, WaiterConfig={"Delay": 15, "MaxAttempts": 80}
        )

    def instance_role(self, name):
        """A role EC2 may assume, with an instance profile of the same name."""
        trust = {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Principal": {"Service": "ec2.amazonaws.com"},
                    "Action": "sts:AssumeRole",
                }
            ],
        }
        self.iam.create_role(
            RoleName=name, AssumeRolePolicyDocument=json.dumps(trust), Tags=self.tags()
        )
        self.cleanups.append(lambda: self.iam.delete_role(RoleName=name))
        self.iam.create_instance_profile(InstanceProfileName=name, Tags=self.tags())
        self.cleanups.append(lambda: self.iam.delete_instance_profile(InstanceProfileName=name))
        self.iam.add_role_to_instance_profile(InstanceProfileName=name, RoleName=name)
        self.cleanups.append(
            lambda: self.iam.remove_role_from_instance_profile(InstanceProfileName=name, RoleName=name)
        )
        return f"arn:aws:iam::{self.account}:instance-profile/{name}"

    def security_group(self, stack_name, vpc):
        """A security group the stack's runtime created, as the management grants require."""
        group = self.ec2.create_security_group(
            GroupName=f"{stack_name}-sg",
            Description="Dry-run launches only",
            VpcId=vpc,
            TagSpecifications=[
                {
                    "ResourceType": "security-group",
                    "Tags": self.tags(
                        [{"Key": "deployment", "Value": stack_name}, {"Key": "managed-by", "Value": "runtime"}]
                    ),
                }
            ],
        )["GroupId"]
        self.cleanups.append(lambda: self.ec2.delete_security_group(GroupId=group))
        return group

    # The management role

    def assume(self, caller_role, stack_name):
        caller = self.sts.assume_role(RoleArn=caller_role, RoleSessionName="caller")["Credentials"]
        as_caller = boto3.session.Session(
            aws_access_key_id=caller["AccessKeyId"],
            aws_secret_access_key=caller["SecretAccessKey"],
            aws_session_token=caller["SessionToken"],
            region_name=self.args.region,
        )
        role = f"arn:aws:iam::{self.account}:role/{stack_name}-management"
        for attempt in range(12):
            try:
                credentials = as_caller.client("sts").assume_role(
                    RoleArn=role, RoleSessionName="management"
                )["Credentials"]
                break
            except ClientError:
                if attempt == 11:
                    raise
                time.sleep(5)
        return boto3.session.Session(
            aws_access_key_id=credentials["AccessKeyId"],
            aws_secret_access_key=credentials["SecretAccessKey"],
            aws_session_token=credentials["SessionToken"],
            region_name=self.args.region,
        )

    def launch(self, session, stack_name, image, subnet, group, profile):
        """A dry-run launch that meets every other condition of the management grants."""
        try:
            session.client("ec2").run_instances(
                DryRun=True,
                ImageId=image,
                InstanceType="t3.micro",
                MinCount=1,
                MaxCount=1,
                SubnetId=subnet,
                SecurityGroupIds=[group],
                IamInstanceProfile={"Arn": profile},
                TagSpecifications=[
                    {
                        "ResourceType": "instance",
                        "Tags": [
                            {"Key": "deployment", "Value": stack_name},
                            {"Key": "managed-by", "Value": "runtime"},
                        ],
                    }
                ],
            )
            return "Launched", {}
        except ClientError as error:
            code = error.response["Error"]["Code"]
            detail = {"message": error.response["Error"]["Message"]}
            encoded = re.search(r"Encoded authorization failure message: (\S+)", detail["message"])
            if encoded:
                decoded = self.sts.decode_authorization_message(EncodedMessage=encoded.group(1))
                detail["decoded"] = json.loads(decoded["DecodedMessage"])
            return code, detail

    def ssm_write(self, session, call, **request):
        try:
            getattr(session.client("ssm"), call)(**request)
            return "Success", {}
        except ClientError as error:
            return error.response["Error"]["Code"], {"message": error.response["Error"]["Message"]}

    def attempts(self, variant, session, stack_name, images, subnet, group, profiles, extra):
        phase = "with-extra-grant" if extra else "policies-only"
        locked = variant == "locked"
        refused_launch = "UnauthorizedOperation" if locked else "DryRunOperation"
        for case, image, profile, expected in [
            ("publisher-image", images["publisher"], profiles["compute"], "DryRunOperation"),
            ("other-owner-image", images["other"], profiles["compute"], refused_launch),
            ("workload-role-to-ec2", images["publisher"], profiles["workload"], refused_launch),
        ]:
            if extra and case == "workload-role-to-ec2":
                continue  # The extra grant allows no PassRole.
            actual, detail = self.launch(session, stack_name, image, subnet, group, profile)
            self.expect(variant, f"{phase}-{case}", expected, actual, detail)

        anchor = f"/_release-trust/{stack_name}"
        writes = [
            ("put-anchor", "put_parameter", {"Name": anchor, "Value": "{}", "Type": "String", "Overwrite": True}),
            (
                "put-deployer-input",
                "put_parameter",
                {"Name": f"/_deployer-inputs/{stack_name}", "Value": "{}", "Type": "String", "Overwrite": True},
            ),
            ("tag-anchor", "add_tags_to_resource", {"ResourceType": "Parameter", "ResourceId": anchor, "Tags": [{"Key": "deployment", "Value": stack_name}]}),
        ]
        for case, call, request in writes:
            if not extra and case != "put-anchor":
                continue  # No management grant reaches these paths; only the guard is in question.
            # Without the guard, the extra grant reaches the paths; put-anchor runs first, so
            # the baseline's tag-anchor finds the parameter it wrote.
            expected = "AccessDeniedException" if locked or not extra else "Success"
            actual, detail = self.ssm_write(session, call, **request)
            if actual == "Success" and call == "put_parameter":
                name = request["Name"]
                self.cleanups.append(lambda name=name: self.ssm.delete_parameter(Name=name))
            self.expect(variant, f"{phase}-{case}", expected, actual, detail)

    def check_anchor(self, stack_name):
        value = self.ssm.get_parameter(Name=f"/_release-trust/{stack_name}")["Parameter"]["Value"]
        anchor = json.loads(value)
        self.record("locked-anchor", anchor)
        ok = (
            anchor.get("protected") is True
            and anchor.get("resourcePrefix") == stack_name
            and anchor.get("inputs") == f"aws-ssm:/_deployer-inputs/{stack_name}"
        )
        self.expect("locked", "anchor-written-by-setup", "True", str(ok), anchor)

    def variant(self, variant, template, caller_role, vpc, subnet, images):
        stack_name = f"{self.args.prefix}-{variant}"
        self.deploy(stack_name, template, caller_role)
        group = self.security_group(stack_name, vpc)
        profiles = {
            "compute": self.instance_role(f"{stack_name}-compute-role"),
            "workload": self.instance_role(f"{stack_name}-api-sa"),
        }
        if variant == "locked":
            self.check_anchor(stack_name)
        time.sleep(15)  # IAM propagation for the roles and profiles.
        session = self.assume(caller_role, stack_name)
        self.attempts(variant, session, stack_name, images, subnet, group, profiles, extra=False)

        # What an extended profile or a customer policy could add to the same role.
        extra = {
            "Version": "2012-10-17",
            "Statement": [
                {
                    "Effect": "Allow",
                    "Action": [
                        "ec2:RunInstances",
                        "ssm:PutParameter",
                        "ssm:DeleteParameter",
                        "ssm:AddTagsToResource",
                    ],
                    "Resource": "*",
                }
            ],
        }
        role = f"{stack_name}-management"
        self.iam.put_role_policy(RoleName=role, PolicyName="extra-grant", PolicyDocument=json.dumps(extra))
        self.cleanups.append(lambda: self.iam.delete_role_policy(RoleName=role, PolicyName="extra-grant"))
        time.sleep(20)
        session = self.assume(caller_role, stack_name)
        self.attempts(variant, session, stack_name, images, subnet, group, profiles, extra=True)

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
        stacks = [
            summary["StackName"]
            for page in self.cfn.get_paginator("list_stacks").paginate()
            for summary in page["StackSummaries"]
            if summary["StackName"].startswith(prefix) and summary["StackStatus"] != "DELETE_COMPLETE"
        ]
        roles = [
            role["RoleName"]
            for page in self.iam.get_paginator("list_roles").paginate()
            for role in page["Roles"]
            if role["RoleName"].startswith(prefix)
        ]
        profiles = [
            profile["InstanceProfileName"]
            for page in self.iam.get_paginator("list_instance_profiles").paginate()
            for profile in page["InstanceProfiles"]
            if profile["InstanceProfileName"].startswith(prefix)
        ]
        parameters = [
            parameter["Name"]
            for path in ["/_release-trust/", "/_deployer-inputs/"]
            for parameter in self.ssm.describe_parameters(
                ParameterFilters=[{"Key": "Name", "Option": "BeginsWith", "Values": [path + prefix]}]
            )["Parameters"]
        ]
        groups = self.ec2.describe_security_groups(
            Filters=[{"Name": f"tag:{self.run_tag['Key']}", "Values": [self.run_tag["Value"]]}]
        )["SecurityGroups"]
        buckets = [
            bucket["Name"]
            for bucket in self.s3.list_buckets()["Buckets"]
            if bucket["Name"].startswith(prefix)
        ]
        left = {
            "tagged": [item["ResourceARN"] for item in tagged],
            "stacks": stacks,
            "roles": roles,
            "instanceProfiles": profiles,
            "parameters": parameters,
            "securityGroups": [group["GroupId"] for group in groups],
            "buckets": buckets,
        }
        self.record("cleanup-audit", {"runTag": self.run_tag, **left})
        return [f"{kind}: {items}" for kind, items in left.items() if items]


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--alien", required=True, help="alien binary with the locks")
    baseline = parser.add_mutually_exclusive_group()
    baseline.add_argument("--baseline-alien", help="alien binary built without the locks")
    baseline.add_argument("--baseline-template", help="a template rendered without the locks")
    parser.add_argument("--prefix", required=True, help="name prefix of everything created")
    parser.add_argument("--tag", required=True, help="KEY=VALUE tag on everything created")
    parser.add_argument("--region", default="us-east-1")
    parser.add_argument(
        "--publisher",
        default=DEFAULT_PUBLISHER,
        help="account whose images stand in for the machine image publisher's",
    )
    parser.add_argument("--evidence", required=True, help="directory for the outputs")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z][a-z0-9-]{2,20}", args.prefix):
        raise SystemExit("--prefix: 3 to 21 lowercase letters, digits and hyphens, starting with a letter")
    if "=" not in args.tag:
        raise SystemExit("--tag must be KEY=VALUE")

    run = Run(args)
    failures = []
    try:
        vpc, subnet = run.default_network()
        publisher = args.publisher
        published = run.ec2.describe_images(
            Owners=[publisher],
            Filters=[
                {"Name": "architecture", "Values": ["x86_64"]},
                {"Name": "root-device-type", "Values": ["ebs"]},
                {"Name": "state", "Values": ["available"]},
            ],
        )["Images"]
        published = [image for image in published if "ImageOwnerAlias" not in image]
        if not published:
            raise SystemExit(f"account {publisher} publishes no x86_64 image without an owner alias")
        images = {
            "publisher": max(published, key=lambda image: image["CreationDate"])["ImageId"],
            "other": run.ssm.get_parameter(Name=OTHER_IMAGE)["Parameter"]["Value"],
        }
        described = run.ec2.describe_images(ImageIds=list(images.values()))["Images"]
        owners = {
            image["ImageId"]: image.get("ImageOwnerAlias", image["OwnerId"]) for image in described
        }
        if owners[images["other"]] == publisher:
            raise SystemExit("the other image must not come from the publisher")
        run.record("images", {"images": images, "owners": owners, "publisher": publisher})

        caller_role = run.caller_role()
        with tempfile.TemporaryDirectory() as scratch:
            scratch = Path(scratch)
            variants = []
            locked_dir = scratch / "locked"
            locked_dir.mkdir()
            settings = run.settings(locked_dir, publisher)
            variants.append(("locked", run.render(args.alien, settings, locked_dir)))
            if args.baseline_alien or args.baseline_template:
                baseline_dir = scratch / "baseline"
                baseline_dir.mkdir()
                if args.baseline_alien:
                    template = run.render(args.baseline_alien, run.settings(baseline_dir, publisher), baseline_dir)
                else:
                    template = Path(args.baseline_template)
                variants.append(("baseline", template))
            for variant, template in variants:
                (run.evidence / f"{variant}-template.json").write_text(template.read_text())
                run.variant(variant, template, caller_role, vpc, subnet, images)
    finally:
        failures += [f"cleanup: {failure}" for failure in run.cleanup()]
        # Tags and deletions reach the listing APIs after a short delay.
        time.sleep(30)
        failures += [f"left behind: {item}" for item in run.audit()]

    failures += [
        f"{result['variant']} {result['case']}: expected {result['expected']}, got {result['actual']}"
        for result in run.results
        if not result["ok"]
    ]
    run.record("summary", {"results": run.results, "failures": failures})
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(f"all {len(run.results)} checks passed; nothing left behind")


if __name__ == "__main__":
    main()
