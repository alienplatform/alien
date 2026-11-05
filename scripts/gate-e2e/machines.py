#!/usr/bin/env python3
"""End-to-end test of `rollMachines` on real AWS.

Creates, all named `<prefix>-<random>` and tagged with --tag where AWS
allows it at creation:
  - the network setup creates for machines: a security group in the
    default VPC, and one of its subnets;
  - the instance profile setup creates for the group, `<p>-compute-general`;
  - the signing settings and ledger parameters;
  - the verifier `<p>-verifier`, configured with VERIFIER_MACHINE_NETWORK.

The verifier binary must name a machine-image publisher; this test builds
it with Amazon's account so Amazon Linux boots, and an Ubuntu image stands
in for "not the publisher's". Then:

  1. a signed release rolls: the verifier creates the launch template and
     the Auto Scaling group from the signed capacity group, with no
     user-data and the instance tags machines read at boot through IMDS
     (capacity group; API URL for development images), and the group
     launches a machine from it;
  2. an image from another owner is refused;
  3. a security group or subnet that isn't setup's is refused (off: the
     same security group written straight into a launch template lands);
  4. an instance type other than the signed one is refused;
  5. desired capacity above the signed maximum is refused by AWS itself;
  6. a second roll adds a template version and keeps the signed bounds;
     it starts no instance refresh (the platform replaces machines);
  7. a publisher image created before the build's minimum is refused.

Registration with a control plane is not part of this test: it needs the
publisher's hardened image and the machine control plane.

Usage (credentials for a test account in the environment):
    scripts/gate-e2e/machines.py \\
        --verifier <alien-gate built with crates/alien-core/machine-image-publisher-aws.txt = 137112412989
                    and machine-image-minimum-created.txt = 2024-06-01T00:00:00Z> \\
        --fixtures target/debug/examples/gate-fixtures \\
        --evidence /tmp/machines-e2e   # resources named $TEST_RESOURCE_PREFIX-<random>
"""

import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import uuid

from aws import REGION, Run as LambdaRun, aws, log

AMAZON = "137112412989"
CANONICAL = "099720109477"
API_URL = "https://machines.example.com"
# What the test's verifier build pins as the oldest image it boots.
MINIMUM_CREATED = "2024-06-01T00:00:00Z"


class Run:
    def __init__(self, args):
        self.args = args
        self.prefix = f"{args.prefix}-{uuid.uuid4().hex[:6]}"
        key, value = args.tag.split("=", 1)
        self.tag_key, self.tag_value = key, value
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="machines-e2e-"))
        self.account = aws("sts", "get-caller-identity")["Account"]
        self.verifier = f"{self.prefix}-verifier"
        self.verifier_role = f"{self.prefix}-verifier"
        self.group = f"{self.prefix}-compute-general"
        self.template = f"{self.group}-lt"
        self.settings_parameter = f"/signed/{self.prefix}/fingerprint"
        self.ledger_parameter = f"/signed/{self.prefix}/state"
        self.install = f"aws:{self.account}:{REGION}:{self.prefix}"
        self.security_group = None
        self.results = []
        self.calls = []

    def tag_spec(self, resource):
        return json.dumps([{"ResourceType": resource, "Tags": [{"Key": self.tag_key, "Value": self.tag_value}]}])

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": ok, "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    # --- setup --------------------------------------------------------------

    def images(self):
        def public(name):
            return aws("ssm", "get-parameter", "--name", name)["Parameter"]["Value"]
        self.ami = public("/aws/service/ami-amazon-linux-latest/al2023-ami-kernel-default-x86_64")
        self.other_ami = public("/aws/service/canonical/ubuntu/server/22.04/stable/current/amd64/hvm/ebs-gp2/ami-id")
        # An Amazon Linux image from 2023: the publisher's, but older than
        # the minimum this test's verifier build boots (MINIMUM_CREATED).
        old = aws("ec2", "describe-images", "--owners", AMAZON, "--filters",
                  "Name=name,Values=al2023-ami-2023.0.2023*-kernel-6.1-x86_64", "Name=architecture,Values=x86_64")["Images"]
        self.old_ami = sorted(old, key=lambda image: image["CreationDate"])[0]["ImageId"]

    def network(self):
        vpc = aws("ec2", "describe-vpcs", "--filters", "Name=isDefault,Values=true")["Vpcs"][0]["VpcId"]
        subnets = aws("ec2", "describe-subnets", "--filters", f"Name=vpc-id,Values={vpc}", "Name=default-for-az,Values=true")["Subnets"]
        subnets.sort(key=lambda subnet: subnet["AvailabilityZone"])
        self.subnet = next(s["SubnetId"] for s in subnets if s["AvailabilityZone"] == f"{REGION}a")
        self.other_subnet = next(s["SubnetId"] for s in subnets if s["SubnetId"] != self.subnet)
        self.security_group = aws("ec2", "create-security-group", "--group-name", f"{self.prefix}-machines",
                                  "--description", "machines e2e", "--vpc-id", vpc,
                                  "--tag-specifications", self.tag_spec("security-group"))["GroupId"]
        self.default_group = aws("ec2", "describe-security-groups", "--filters", f"Name=vpc-id,Values={vpc}",
                                 "Name=group-name,Values=default")["SecurityGroups"][0]["GroupId"]

    def roles(self):
        tags = json.dumps([{"Key": self.tag_key, "Value": self.tag_value}])
        ec2_trust = json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "ec2.amazonaws.com"}, "Action": "sts:AssumeRole"}]})
        aws("iam", "create-role", "--role-name", self.group, "--assume-role-policy-document", ec2_trust, "--tags", tags)
        aws("iam", "create-instance-profile", "--instance-profile-name", self.group, "--tags", tags)
        aws("iam", "add-role-to-instance-profile", "--instance-profile-name", self.group, "--role-name", self.group)
        lambda_trust = json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]})
        aws("iam", "create-role", "--role-name", self.verifier_role, "--assume-role-policy-document", lambda_trust, "--tags", tags)
        aws("iam", "attach-role-policy", "--role-name", self.verifier_role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole")
        parameters = f"arn:aws:ssm:{REGION}:{self.account}:parameter"
        policy = {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": "ssm:GetParameter", "Resource": f"{parameters}/signed/{self.prefix}/*"},
            {"Effect": "Allow", "Action": "ssm:PutParameter", "Resource": f"{parameters}{self.ledger_parameter}"},
            {"Effect": "Allow", "Action": ["ec2:DescribeImages", "ec2:DescribeLaunchTemplates", "ec2:DescribeLaunchTemplateVersions",
                                           "ec2:CreateLaunchTemplate", "ec2:CreateLaunchTemplateVersion", "ec2:RunInstances",
                                           "ec2:CreateTags", "ec2:Describe*",
                                           "autoscaling:DescribeAutoScalingGroups", "autoscaling:CreateAutoScalingGroup",
                                           "autoscaling:UpdateAutoScalingGroup"],
             "Resource": "*"},
            {"Effect": "Allow", "Action": "iam:PassRole", "Resource": f"arn:aws:iam::{self.account}:role/{self.group}"},
            {"Effect": "Allow", "Action": "iam:CreateServiceLinkedRole", "Resource": "*",
             "Condition": {"StringEquals": {"iam:AWSServiceName": "autoscaling.amazonaws.com"}}},
        ]}
        aws("iam", "put-role-policy", "--role-name", self.verifier_role, "--policy-name", "verifier", "--policy-document", json.dumps(policy))
        time.sleep(15)  # IAM propagation

    def fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), self.prefix, self.install,
                        f"{self.account}.dkr.ecr.{REGION}.amazonaws.com/unused@sha256:{'0' * 64}",
                        f"{self.account}.dkr.ecr.{REGION}.amazonaws.com/unused@sha256:{'1' * 64}"],
                       check=True, capture_output=True)
        self.statements = {path.stem: json.loads(path.read_text()) for path in out.glob("*.json")}
        for name, value in [(self.settings_parameter, self.statements["settings"]), (self.ledger_parameter, self.statements["ledger"])]:
            aws("ssm", "put-parameter", "--name", name, "--type", "String", "--value", json.dumps(value),
                "--tags", f"Key={self.tag_key},Value={self.tag_value}")

    def create_verifier(self):
        # The Lambda kit packages the verifier the same way.
        packer = LambdaRun.__new__(LambdaRun)
        packer.args, packer.work = self.args, self.work
        zip_path = packer.verifier_zip()
        network = {"subnetIds": [self.subnet], "securityGroupIds": [self.security_group]}
        aws("lambda", "create-function", "--function-name", self.verifier, "--runtime", "provided.al2023",
            "--handler", "bootstrap", "--architectures", "x86_64", "--zip-file", f"fileb://{zip_path}",
            "--role", f"arn:aws:iam::{self.account}:role/{self.verifier_role}", "--memory-size", "256", "--timeout", "300",
            "--environment", json.dumps({"Variables": {"VERIFIER_CLOUD": "aws", "VERIFIER_MACHINE_NETWORK": json.dumps(network)}}),
            "--tags", f"{self.tag_key}={self.tag_value}")
        for _ in range(60):
            if aws("lambda", "get-function-configuration", "--function-name", self.verifier).get("State") == "Active":
                return
            time.sleep(5)
        raise RuntimeError("the verifier did not become active")

    # --- calls --------------------------------------------------------------

    def invoke(self, **overrides):
        payload = {"rollMachines": {
            "cluster": "compute", "group": "general",
            "release": self.statements["release-70-machines"], "signerLists": [self.statements["signer-list"]],
            "machineImage": self.ami, "apiUrl": API_URL, **overrides}}
        out = self.work / f"out-{uuid.uuid4().hex[:6]}.json"
        response = aws("lambda", "invoke", "--function-name", self.verifier, "--cli-binary-format", "raw-in-base64-out",
                       "--payload", json.dumps(payload), str(out))
        body = json.loads(out.read_text() or "null")
        ok = not response.get("FunctionError")
        self.calls.append({"request": payload, "ok": ok, "response": body})
        return ok, (body if ok else body.get("errorMessage", "") if isinstance(body, dict) else str(body))

    def template_version(self, version):
        return aws("ec2", "describe-launch-template-versions", "--launch-template-name", self.template,
                   "--versions", str(version))["LaunchTemplateVersions"][0]["LaunchTemplateData"]

    def asg(self):
        groups = aws("autoscaling", "describe-auto-scaling-groups", "--auto-scaling-group-names", self.group)["AutoScalingGroups"]
        return groups[0] if groups else None

    def expect_refused(self, name, reason, **overrides):
        versions_before = self.versions()
        ok, body = self.invoke(**overrides)
        unchanged = self.versions() == versions_before
        self.record(name, not ok and reason in str(body) and unchanged, f"verifier ok={ok} template unchanged={unchanged} reason={str(body)[:400]}")

    def versions(self):
        result = aws("ec2", "describe-launch-template-versions", "--launch-template-name", self.template, check=False, parse=False)
        if result.returncode != 0:
            return []
        return sorted(v["VersionNumber"] for v in json.loads(result.stdout)["LaunchTemplateVersions"])

    def run(self):
        self.images()
        self.network()
        self.roles()
        self.fixtures()
        self.create_verifier()
        log(f"ready: {self.verifier}, network {self.subnet} {self.security_group}, image {self.ami}")

        # 2-4 before the first roll: nothing exists, and nothing may be created.
        self.expect_refused("2 an image from another owner (on)", f"is owned by {CANONICAL}, not the publisher {AMAZON}", machineImage=self.other_ami)
        self.expect_refused("3 a security group that isn't setup's (on)",
                            f"security group {self.default_group} is not one setup created", securityGroupIds=[self.security_group, self.default_group])
        self.expect_refused("3 a subnet that isn't setup's (on)", f"subnet {self.other_subnet} is not one setup created", subnetIds=[self.other_subnet])
        self.expect_refused("4 an instance type other than the signed one (on)", "is not the signed t3.micro", instanceType="m5.24xlarge")
        self.expect_refused("7 a publisher image older than the build's minimum (on)", "the oldest image this build boots", machineImage=self.old_ami)
        self.record("2-4 nothing was created by a refused roll", self.asg() is None and self.versions() == [], f"asg={self.asg()} versions={self.versions()}")

        # 1. The signed roll.
        ok, body = self.invoke(securityGroupIds=[self.security_group], subnetIds=[self.subnet])
        self.record("1 a signed release rolls", ok and body.get("version") == 1 and body.get("group") == self.group and "instanceRefreshId" not in body, str(body)[:400])
        if ok:
            aws("ec2", "create-tags", "--resources", body["launchTemplateId"], "--tags", f"Key={self.tag_key},Value={self.tag_value}")
        data = self.template_version(1)
        tags = {t["Key"]: t["Value"] for spec in data.get("TagSpecifications", []) if spec["ResourceType"] == "instance" for t in spec["Tags"]}
        self.record("1 the template is the signed group's, with what machines read at boot", data["ImageId"] == self.ami and data["InstanceType"] == "t3.micro"
                    and data["SecurityGroupIds"] == [self.security_group]
                    and data["IamInstanceProfile"]["Arn"] == f"arn:aws:iam::{self.account}:instance-profile/{self.group}"
                    and data["MetadataOptions"]["HttpTokens"] == "required" and data["MetadataOptions"].get("InstanceMetadataTags") == "enabled"
                    and "UserData" not in data and tags == {"HorizonCapacityGroup": "general", "HorizonApiUrl": API_URL},
                    json.dumps(data)[:700])
        group = self.asg()
        self.record("1 the group has the signed bounds and setup's subnet", group is not None and group["MinSize"] == 1 and group["MaxSize"] == 1
                    and group["VPCZoneIdentifier"] == self.subnet and group["LaunchTemplate"]["Version"] == "1",
                    json.dumps({k: group.get(k) for k in ("MinSize", "MaxSize", "DesiredCapacity", "VPCZoneIdentifier", "LaunchTemplate")}) if group else "no group")
        aws("autoscaling", "create-or-update-tags", "--tags",
            f"ResourceId={self.group},ResourceType=auto-scaling-group,Key={self.tag_key},Value={self.tag_value},PropagateAtLaunch=true", check=False, parse=False)
        ledger = json.loads(aws("ssm", "get-parameter", "--name", self.ledger_parameter)["Parameter"]["Value"])
        self.record("1 the ledger rose to release 70", ledger.get("release", {}).get("sequence") == 70, json.dumps(ledger)[:300])

        instance = None
        for _ in range(40):
            instances = (self.asg() or {}).get("Instances", [])
            if instances:
                instance = aws("ec2", "describe-instances", "--instance-ids", instances[0]["InstanceId"])["Reservations"][0]["Instances"][0]
                break
            time.sleep(6)
        instance_tags = {t["Key"]: t["Value"] for t in (instance or {}).get("Tags", [])}
        self.record("1 the group launched a machine from the signed template", instance is not None and instance["ImageId"] == self.ami
                    and instance["SubnetId"] == self.subnet and [g["GroupId"] for g in instance["SecurityGroups"]] == [self.security_group]
                    and instance["InstanceType"] == "t3.micro"
                    and instance_tags.get("HorizonCapacityGroup") == "general" and instance_tags.get("HorizonApiUrl") == API_URL
                    and instance["MetadataOptions"].get("InstanceMetadataTags") == "enabled",
                    json.dumps({k: instance.get(k) for k in ("InstanceId", "ImageId", "SubnetId", "InstanceType", "SecurityGroups", "Tags", "MetadataOptions")}) if instance else "no instance")

        # 3 off: the same security group written straight into the template lands.
        landed = aws("ec2", "create-launch-template-version", "--launch-template-name", self.template, "--source-version", "1",
                     "--launch-template-data", json.dumps({"SecurityGroupIds": [self.default_group]}), check=False, parse=False)
        self.record("3 a security group that isn't setup's (off: written straight to EC2)", landed.returncode == 0 and self.template_version(2)["SecurityGroupIds"] == [self.default_group],
                    landed.stderr.strip()[:300] or "landed")

        # 5. AWS holds the signed maximum.
        result = aws("autoscaling", "set-desired-capacity", "--auto-scaling-group-name", self.group, "--desired-capacity", "5", check=False, parse=False)
        self.record("5 desired capacity above the signed maximum is refused by AWS", result.returncode != 0 and "ValidationError" in result.stderr, result.stderr.strip()[:300])

        # 6. A second roll.
        ok, body = self.invoke()
        group = self.asg()
        refreshes = aws("autoscaling", "describe-instance-refreshes", "--auto-scaling-group-name", self.group)["InstanceRefreshes"]
        self.record("6 a second roll adds a version, keeps the bounds, starts no refresh", ok and body.get("version") == 3
                    and "instanceRefreshId" not in body and not refreshes
                    and group["LaunchTemplate"]["Version"] == "3" and group["MinSize"] == 1 and group["MaxSize"] == 1
                    and self.template_version(3)["SecurityGroupIds"] == [self.security_group], f"{str(body)[:300]} refreshes={refreshes}")

    # --- teardown -----------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        aws("autoscaling", "delete-auto-scaling-group", "--auto-scaling-group-name", self.group, "--force-delete", check=False, parse=False)
        for _ in range(60):
            if self.asg() is None:
                break
            time.sleep(10)
        aws("ec2", "delete-launch-template", "--launch-template-name", self.template, check=False, parse=False)
        aws("lambda", "delete-function", "--function-name", self.verifier, check=False, parse=False)
        aws("logs", "delete-log-group", "--log-group-name", f"/aws/lambda/{self.verifier}", check=False, parse=False)
        for parameter in (self.settings_parameter, self.ledger_parameter):
            aws("ssm", "delete-parameter", "--name", parameter, check=False, parse=False)
        aws("iam", "remove-role-from-instance-profile", "--instance-profile-name", self.group, "--role-name", self.group, check=False, parse=False)
        aws("iam", "delete-instance-profile", "--instance-profile-name", self.group, check=False, parse=False)
        aws("iam", "delete-role", "--role-name", self.group, check=False, parse=False)
        aws("iam", "delete-role-policy", "--role-name", self.verifier_role, "--policy-name", "verifier", check=False, parse=False)
        aws("iam", "detach-role-policy", "--role-name", self.verifier_role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole", check=False, parse=False)
        aws("iam", "delete-role", "--role-name", self.verifier_role, check=False, parse=False)
        if self.security_group:
            for _ in range(30):
                if aws("ec2", "delete-security-group", "--group-id", self.security_group, check=False, parse=False).returncode == 0:
                    break
                time.sleep(10)

    def audit(self):
        left = []
        if self.asg() is not None:
            left.append(f"group {self.group}")
        if self.versions():
            left.append(f"template {self.template}")
        running = aws("ec2", "describe-instances", "--filters", f"Name=tag:aws:autoscaling:groupName,Values={self.group}",
                      "Name=instance-state-name,Values=pending,running,stopping,stopped")["Reservations"]
        left += [f"instance {i['InstanceId']}" for r in running for i in r["Instances"]]
        if self.security_group and aws("ec2", "describe-security-groups", "--group-ids", self.security_group, check=False, parse=False).returncode == 0:
            left.append(f"security group {self.security_group}")
        for role in (self.group, self.verifier_role):
            if aws("iam", "get-role", "--role-name", role, check=False, parse=False).returncode == 0:
                left.append(f"role {role}")
        if aws("lambda", "get-function", "--function-name", self.verifier, check=False, parse=False).returncode == 0:
            left.append(f"function {self.verifier}")
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--verifier", required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--prefix", default=os.environ.get("TEST_RESOURCE_PREFIX"),
                        help="prefix for every resource name (default: $TEST_RESOURCE_PREFIX)")
    parser.add_argument("--tag", help="key=value tag for every resource (default: <prefix>=true)")
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    if not args.prefix:
        parser.error("give --prefix or set TEST_RESOURCE_PREFIX")
    args.tag = args.tag or f"{args.prefix}=true"
    run = Run(args)
    log(f"prefix {run.prefix}")
    try:
        run.run()
    except Exception as error:
        run.record("the run itself", False, f"{type(error).__name__}: {error}")
    finally:
        (run.evidence / "results.json").write_text(json.dumps(run.results, indent=2))
        (run.evidence / "calls.json").write_text(json.dumps(run.calls, indent=2))
        if not args.keep:
            run.cleanup()
            left = run.audit()
            (run.evidence / "audit.json").write_text(json.dumps(left))
            log(f"left after cleanup: {left or 'nothing'}")
    failed = [result for result in run.results if not result["pass"]]
    log(f"{len(run.results) - len(failed)}/{len(run.results)} passed")
    if failed or not run.results:
        sys.exit(1)


if __name__ == "__main__":
    main()
