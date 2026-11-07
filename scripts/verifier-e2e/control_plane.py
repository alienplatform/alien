#!/usr/bin/env python3
"""A deployment's update of a Lambda worker on real AWS, protected and unprotected.

Checks the control plane's side of the verifier: the stack executor and AWS worker controller,
running as an install's management role, update the function through the install's verifier
when the install is protected, and by writing it themselves when it isn't.

Creates two installs, `<p>-p` (protected) and `<p>-u` (unprotected), each as setup would:
  - the workload's role `<i>-execution-sa` and the function `<i>-api` on image A;
  - the management role `<i>-management`, which the caller may assume, allowed Lambda writes on
    the install's functions; the protected one also carries `<i>-management-limits`, the policy
    protected setup renders, which denies them;
  - protected only: the signing settings and ledger under `/signed/<i>/`, and the verifier
    `<i>-verifier` (Lambda, provided.al2023) with the policy setup renders for it.

Then, as each management role, runs `protected_worker_update update` (alien-infra example) to
move `api` to image B, and checks with the caller's own credentials:
  1. protected: the update succeeds, the function runs B, the verifier logged the write, and
     the management role itself is denied UpdateFunctionCode (the path the controller used to
     take);
  2. protected: a release whose signer list isn't the one setup pinned is refused with the
     verifier's reason, and the function keeps running B;
  3. unprotected: the update succeeds by writing the function directly; no verifier exists.

Everything is named `<p>-...`, tagged with --tag, deleted at the end, and the run fails if
anything named `<p>` is left.

    scripts/verifier-e2e/control_plane.py \\
        --verifier target/x86_64-unknown-linux-musl/debug/alien-verifier \\
        --driver target/debug/examples/protected_worker_update \\
        --prefix e2e-cp --tag purpose=control-plane-e2e --evidence /tmp/control-plane-e2e

Requires administrator credentials for a disposable test account in the environment, the AWS
CLI and Docker.
"""

import argparse
import datetime
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import uuid
import zipfile

REGION = "us-east-1"


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


def aws(*args, check=True, env=None):
    result = subprocess.run(["aws", "--region", REGION, "--output", "json", *args],
                            capture_output=True, text=True, env=env)
    if check and result.returncode != 0:
        raise RuntimeError(f"aws {' '.join(args[:2])} failed: {result.stderr.strip()}")
    return result


def aws_json(*args, **kwargs):
    out = aws(*args, **kwargs).stdout
    return json.loads(out) if out.strip() else {}


class Run:
    def __init__(self, args):
        self.args = args
        self.prefix = f"{args.prefix}-{uuid.uuid4().hex[:4]}"
        self.tag_key, self.tag_value = args.tag.split("=", 1)
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="control-plane-e2e-"))
        self.account = aws_json("sts", "get-caller-identity")["Account"]
        self.caller = aws_json("sts", "get-caller-identity")["Arn"]
        self.registry = f"{self.account}.dkr.ecr.{REGION}.amazonaws.com"
        self.repo = f"{self.registry}/{self.prefix}"
        # Lambda takes a signer list only within 90 days; both driver runs must sign the same one.
        self.not_after = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=30)) \
            .replace(microsecond=0).isoformat()
        self.results = []
        self.created = []

    def tags(self):
        return json.dumps([{"Key": self.tag_key, "Value": self.tag_value}])

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def driver(self, command, install, env=None, check=True):
        variables = {**os.environ, "PREFIX": install, "NOT_AFTER": self.not_after,
                     "ACCOUNT": self.account, "REGION": REGION, "AWS_REGION": REGION, **(env or {})}
        result = subprocess.run([self.args.driver, command], capture_output=True, text=True, env=variables)
        (self.evidence / f"driver-{install}-{command}-{uuid.uuid4().hex[:4]}.log").write_text(
            result.stdout + "\n--- stderr ---\n" + result.stderr)
        if check and result.returncode != 0:
            raise RuntimeError(f"driver {command} failed: {result.stderr[-2000:]}")
        return result

    # --- setup ----------------------------------------------------------------

    def push_images(self):
        aws("ecr", "create-repository", "--repository-name", self.prefix, "--tags", self.tags())
        self.created.append(("ecr", self.prefix))
        password = aws("ecr", "get-login-password").stdout
        subprocess.run(["docker", "login", "--username", "AWS", "--password-stdin", self.registry],
                       input=password.encode(), check=True, capture_output=True)
        self.images = {}
        for tag, source in [("a", "public.ecr.aws/docker/library/busybox:1.36"),
                            ("b", "public.ecr.aws/docker/library/alpine:3.20")]:
            for step in (["pull", "--platform", "linux/arm64", source], ["tag", source, f"{self.repo}:{tag}"],
                         ["push", f"{self.repo}:{tag}"]):
                subprocess.run(["docker", *step], check=True, capture_output=True)
            digest = aws_json("ecr", "describe-images", "--repository-name", self.prefix,
                              "--image-ids", f"imageTag={tag}")["imageDetails"][0]["imageDigest"]
            self.images[tag] = f"{self.repo}@{digest}"

    def role(self, name, trust, managed=(), inline=None):
        aws("iam", "create-role", "--role-name", name, "--assume-role-policy-document", json.dumps(trust),
            "--tags", self.tags())
        self.created.append(("role", name))
        for arn in managed:
            aws("iam", "attach-role-policy", "--role-name", name, "--policy-arn", arn)
        if inline:
            aws("iam", "put-role-policy", "--role-name", name, "--policy-name", "inline",
                "--policy-document", json.dumps(inline))

    def wait_active(self, function):
        for _ in range(60):
            configuration = aws_json("lambda", "get-function-configuration", "--function-name", function)
            if configuration.get("State") == "Active" and configuration.get("LastUpdateStatus") in (None, "Successful"):
                return
            time.sleep(5)
        raise RuntimeError(f"{function} did not become active")

    def verifier_zip(self):
        stripped = self.work / "bootstrap"
        subprocess.run(["strip", "-o", str(stripped), self.args.verifier], check=True)
        buffer = io.BytesIO()
        with zipfile.ZipFile(buffer, "w", zipfile.ZIP_DEFLATED) as archive:
            info = zipfile.ZipInfo("bootstrap")
            info.external_attr = 0o755 << 16
            archive.writestr(info, stripped.read_bytes())
        path = self.work / "verifier.zip"
        path.write_bytes(buffer.getvalue())
        return path

    def install(self, install, protected):
        lambda_trust = {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]}
        basic = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
        self.role(f"{install}-execution-sa", lambda_trust, managed=[basic])
        policies = json.loads(self.driver("policies", install).stdout)
        (self.evidence / f"{install}-policies.json").write_text(json.dumps(policies, indent=2))
        management = {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": "lambda:*", "Resource": [
                f"arn:aws:lambda:{REGION}:{self.account}:function:{install}-*",
                f"arn:aws:lambda:{REGION}:{self.account}:function:{install}-*:*"]},
            {"Effect": "Allow", "Action": ["logs:CreateLogGroup", "logs:PutRetentionPolicy", "logs:DescribeLogGroups"],
             "Resource": "*"},
            {"Effect": "Allow", "Action": "iam:PassRole", "Resource": f"arn:aws:iam::{self.account}:role/{install}-*"},
            {"Effect": "Allow", "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer"], "Resource": "*"}]}
        caller_trust = {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Principal": {"AWS": f"arn:aws:iam::{self.account}:root"}, "Action": "sts:AssumeRole"}]}
        self.role(f"{install}-management", caller_trust, inline=management)
        if protected:
            limits = aws_json("iam", "create-policy", "--policy-name", f"{install}-management-limits",
                              "--policy-document", json.dumps(policies["managementLimits"]), "--tags", self.tags())
            self.created.append(("policy", limits["Policy"]["Arn"]))
            aws("iam", "attach-role-policy", "--role-name", f"{install}-management", "--policy-arn", limits["Policy"]["Arn"])
            fixtures = json.loads(self.driver("fixtures", install).stdout)
            for name, value in [(f"/signed/{install}/fingerprint", fixtures["settings"]),
                                (f"/signed/{install}/state", fixtures["ledger"])]:
                aws("ssm", "put-parameter", "--name", name, "--type", "String", "--value", json.dumps(value),
                    "--tags", f"Key={self.tag_key},Value={self.tag_value}")
                self.created.append(("parameter", name))
            self.role(f"{install}-verifier", lambda_trust, managed=[basic], inline=policies["verifier"])
        time.sleep(15)  # IAM propagation
        aws("lambda", "create-function", "--function-name", f"{install}-api", "--package-type", "Image",
            "--code", f"ImageUri={self.images['a']}", "--role", f"arn:aws:iam::{self.account}:role/{install}-execution-sa",
            "--architectures", "arm64", "--memory-size", "128", "--timeout", "3",
            "--tags", f"{self.tag_key}={self.tag_value}")
        self.created.append(("function", f"{install}-api"))
        if protected:
            aws("lambda", "create-function", "--function-name", f"{install}-verifier", "--runtime", "provided.al2023",
                "--handler", "bootstrap", "--architectures", "x86_64", "--zip-file", f"fileb://{self.zip}",
                "--role", f"arn:aws:iam::{self.account}:role/{install}-verifier", "--memory-size", "256",
                "--timeout", "300", "--environment", json.dumps({"Variables": {"VERIFIER_CLOUD": "aws", "RUST_LOG": "info"}}),
                "--tags", f"{self.tag_key}={self.tag_value}")
            self.created.append(("function", f"{install}-verifier"))
            self.created.append(("log-group", f"/aws/lambda/{install}-verifier"))
            self.wait_active(f"{install}-verifier")
        self.created.append(("log-group", f"/aws/lambda/{install}-api"))
        self.wait_active(f"{install}-api")

    def management_env(self, install):
        credentials = aws_json("sts", "assume-role", "--role-arn", f"arn:aws:iam::{self.account}:role/{install}-management",
                               "--role-session-name", "control-plane-e2e")["Credentials"]
        return {"AWS_ACCESS_KEY_ID": credentials["AccessKeyId"], "AWS_SECRET_ACCESS_KEY": credentials["SecretAccessKey"],
                "AWS_SESSION_TOKEN": credentials["SessionToken"], "AWS_REGION": REGION}

    def image(self, install):
        return aws_json("lambda", "get-function", "--function-name", f"{install}-api")["Code"]["ImageUri"]

    # --- cases ------------------------------------------------------------------

    def protected(self):
        install = f"{self.prefix}-p"
        management = self.management_env(install)
        update = {**management, "FROM_IMAGE": self.images["a"], "TO_IMAGE": self.images["b"], "PROTECTED": "1"}
        result = self.driver("update", install, update, check=False)
        outcome = json.loads(result.stdout.strip().splitlines()[-1]) if result.stdout.strip() else {}
        image = self.image(install)
        time.sleep(10)
        events = aws_json("logs", "filter-log-events", "--log-group-name", f"/aws/lambda/{install}-verifier",
                          "--filter-pattern", '"applied the signed function"', check=False).get("events", [])
        self.record("1 protected: the update goes through the verifier and lands",
                    result.returncode == 0 and outcome.get("outcome") == "synced" and image == self.images["b"] and events,
                    f"driver={outcome} image={image.rsplit('@', 1)[-1][:19]} verifier-log-lines={len(events)}")
        direct = aws("lambda", "update-function-code", "--function-name", f"{install}-api",
                     "--image-uri", self.images["a"], check=False, env={**os.environ, **management})
        self.record("1 protected: management itself may not UpdateFunctionCode",
                    direct.returncode != 0 and "AccessDenied" in direct.stderr and "explicit deny" in direct.stderr,
                    direct.stderr.strip()[:300])

        foreign = {**update, "FROM_IMAGE": self.images["b"], "TO_IMAGE": self.images["a"],
                   "NOT_AFTER": (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=20))
                   .replace(microsecond=0).isoformat()}
        result = self.driver("update", install, foreign, check=False)
        outcome = json.loads(result.stdout.strip().splitlines()[-1]) if result.stdout.strip() else {}
        image = self.image(install)
        self.record("2 protected: a release under a signer list setup didn't pin is refused, with the reason",
                    result.returncode != 0 and "RELEASE_REFUSED_BY_VERIFIER" in json.dumps(outcome)
                    and "does not match the pinned fingerprint" in result.stdout + result.stderr
                    and image == self.images["b"],
                    f"driver={json.dumps(outcome)[:400]} image={image.rsplit('@', 1)[-1][:19]}")

    def unprotected(self):
        install = f"{self.prefix}-u"
        update = {**self.management_env(install), "FROM_IMAGE": self.images["a"], "TO_IMAGE": self.images["b"],
                  "PROTECTED": "0"}
        result = self.driver("update", install, update, check=False)
        outcome = json.loads(result.stdout.strip().splitlines()[-1]) if result.stdout.strip() else {}
        image = self.image(install)
        verifier = aws("lambda", "get-function", "--function-name", f"{install}-verifier", check=False)
        self.record("3 unprotected: the update writes the function itself, with no verifier",
                    result.returncode == 0 and outcome.get("outcome") == "synced" and image == self.images["b"]
                    and verifier.returncode != 0,
                    f"driver={outcome} image={image.rsplit('@', 1)[-1][:19]}")

    # --- cleanup ----------------------------------------------------------------

    def cleanup(self):
        for kind, name in reversed(self.created):
            if kind == "function":
                aws("lambda", "delete-function", "--function-name", name, check=False)
            elif kind == "log-group":
                aws("logs", "delete-log-group", "--log-group-name", name, check=False)
            elif kind == "parameter":
                aws("ssm", "delete-parameter", "--name", name, check=False)
            elif kind == "ecr":
                aws("ecr", "delete-repository", "--repository-name", name, "--force", check=False)
            elif kind == "policy":
                for role in json.loads(aws("iam", "list-entities-for-policy", "--policy-arn", name, check=False).stdout or "{}").get("PolicyRoles", []):
                    aws("iam", "detach-role-policy", "--role-name", role["RoleName"], "--policy-arn", name, check=False)
                aws("iam", "delete-policy", "--policy-arn", name, check=False)
            elif kind == "role":
                for policy in aws_json("iam", "list-attached-role-policies", "--role-name", name, check=False).get("AttachedPolicies", []):
                    aws("iam", "detach-role-policy", "--role-name", name, "--policy-arn", policy["PolicyArn"], check=False)
                for policy in aws_json("iam", "list-role-policies", "--role-name", name, check=False).get("PolicyNames", []):
                    aws("iam", "delete-role-policy", "--role-name", name, "--policy-name", policy, check=False)
                aws("iam", "delete-role", "--role-name", name, check=False)

    def audit(self):
        left = []
        functions = aws_json("lambda", "list-functions")["Functions"]
        left += [f["FunctionName"] for f in functions if f["FunctionName"].startswith(self.prefix)]
        roles = aws_json("iam", "list-roles")["Roles"]
        left += [r["RoleName"] for r in roles if r["RoleName"].startswith(self.prefix)]
        policies = aws_json("iam", "list-policies", "--scope", "Local")["Policies"]
        left += [p["PolicyName"] for p in policies if p["PolicyName"].startswith(self.prefix)]
        parameters = aws_json("ssm", "describe-parameters", "--parameter-filters",
                              f"Key=Name,Option=BeginsWith,Values=/signed/{self.prefix}")["Parameters"]
        left += [p["Name"] for p in parameters]
        repos = aws_json("ecr", "describe-repositories")["repositories"]
        left += [r["repositoryName"] for r in repos if r["repositoryName"].startswith(self.prefix)]
        groups = aws_json("logs", "describe-log-groups", "--log-group-name-prefix", f"/aws/lambda/{self.prefix}")["logGroups"]
        left += [g["logGroupName"] for g in groups]
        self.record("cleanup: nothing named after the run is left", not left, f"left={left}")

    def run(self):
        log(f"run {self.prefix}")
        try:
            self.zip = self.verifier_zip()
            self.push_images()
            self.install(f"{self.prefix}-p", protected=True)
            self.install(f"{self.prefix}-u", protected=False)
            self.protected()
            self.unprotected()
        finally:
            for install in (f"{self.prefix}-p",):
                out = aws("logs", "filter-log-events", "--log-group-name", f"/aws/lambda/{install}-verifier", check=False)
                (self.evidence / f"{install}-verifier-log.json").write_text(out.stdout)
            self.cleanup()
            self.audit()
            (self.evidence / "results.json").write_text(json.dumps(self.results, indent=2))
        failed = [r for r in self.results if not r["pass"]]
        log(f"{len(self.results) - len(failed)}/{len(self.results)} passed")
        return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--verifier", required=True)
    parser.add_argument("--driver", required=True)
    parser.add_argument("--prefix", default=os.environ.get("TEST_RESOURCE_PREFIX", "e2e-cp"))
    parser.add_argument("--tag", default="purpose=control-plane-e2e")
    parser.add_argument("--evidence", required=True)
    sys.exit(Run(parser.parse_args()).run())


if __name__ == "__main__":
    main()
