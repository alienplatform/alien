#!/usr/bin/env python3
"""End-to-end test of the verifier on real AWS.

Creates, all named `<prefix>-<random>` and tagged with --tag:
  - an ECR repository with two small images;
  - the workload's execution role `<p>-execution-sa` and the verifier's role;
  - the workload function `<p>-api`, as setup would create it;
  - the signing settings `/signed/<p>/fingerprint` and the ledger
    `/signed/<p>/state`, as setup writes them;
  - the verifier `<p>-verifier`, a Lambda function on provided.al2023.

Then calls the verifier the way the control plane would and checks every
result by reading the function and the ledger back with the caller's own
credentials. Each attack runs "off" where that means something (the same
change made straight through the Lambda API, as management of an
unprotected install can, lands) and "on" (the verifier refuses it, with the
reason, and the function keeps running the signed release):

  1. a signed release with signed config lands exactly as signed;
  2. an unsigned release is refused;
  3. a binding redirect is refused: from the control plane's values, and
     from a signed config naming a binding the install derives;
  4. LD_PRELOAD from a vault is refused: a release whose secret is
     LD_PRELOAD, and the secrets list or LD_PRELOAD from the control plane;
  5. a `dev` release to a `production` install is refused;
  6. a release signed by a key not on the list is refused;
  7. a newer release lands; a fresh verifier reading the ledger refuses the
     older one;
  8. changes made behind the verifier (env, log destination) are caught by
     checkRelease and undone by applyRelease;
  9. deleting the settings parameter after acceptance refuses (sticky);
 10. a verifier role that may read every parameter sees two candidate
     settings and refuses (with the role scoped, the same install works).

Usage (credentials for a test account in the environment):
    scripts/gate-e2e/aws.py \\
        --verifier target/x86_64-unknown-linux-musl/debug/alien-gate \\
        --fixtures target/debug/examples/gate-fixtures \\
        --evidence /tmp/verifier-e2e   # resources named $TEST_RESOURCE_PREFIX-<random>
"""

import argparse
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


def aws(*args, check=True, parse=True):
    result = subprocess.run(["aws", "--region", REGION, "--output", "json", *args], capture_output=True, text=True)
    if check and result.returncode != 0:
        raise RuntimeError(f"aws {' '.join(args[:2])} failed: {result.stderr.strip()}")
    if not parse:
        return result
    return json.loads(result.stdout) if result.stdout.strip() else {}


def same_value(found, expected):
    """Env values compare as JSON when both are JSON (binding values are)."""
    try:
        return json.loads(found) == json.loads(expected)
    except (TypeError, ValueError):
        return found == expected


class Run:
    def __init__(self, args):
        self.args = args
        self.prefix = f"{args.prefix}-{uuid.uuid4().hex[:6]}"
        key, value = args.tag.split("=", 1)
        self.tag_key, self.tag_value = key, value
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="verifier-e2e-"))
        self.account = aws("sts", "get-caller-identity")["Account"]
        self.registry = f"{self.account}.dkr.ecr.{REGION}.amazonaws.com"
        self.repo = f"{self.registry}/{self.prefix}"
        self.workload = f"{self.prefix}-api"
        self.verifier = f"{self.prefix}-verifier"
        self.exec_role = f"{self.prefix}-execution-sa"
        self.verifier_role = f"{self.prefix}-verifier"
        self.settings_parameter = f"/signed/{self.prefix}/fingerprint"
        self.ledger_parameter = f"/signed/{self.prefix}/state"
        # A second place a verifier named `<p>-verifier` could find settings.
        self.decoy_parameter = f"/signed/{self.verifier}/fingerprint"
        self.install = f"aws:{self.account}:{REGION}:{self.prefix}"
        self.results = []
        self.calls = []
        self.config = None

    def tags_list(self):
        return json.dumps([{"Key": self.tag_key, "Value": self.tag_value}])

    # --- setup ----------------------------------------------------------------

    def docker(self, *args, input=None):
        subprocess.run(["docker", *args], check=True, capture_output=True, input=input)

    def push_images(self):
        aws("ecr", "create-repository", "--repository-name", self.prefix, "--tags", self.tags_list())
        password = aws("ecr", "get-login-password", parse=False).stdout
        self.docker("login", "--username", "AWS", "--password-stdin", self.registry, input=password.encode())
        digests = {}
        for tag, source in [("a", "public.ecr.aws/docker/library/busybox:1.36"), ("b", "public.ecr.aws/docker/library/alpine:3.20")]:
            self.docker("pull", "--platform", "linux/amd64", source)
            self.docker("tag", source, f"{self.repo}:{tag}")
            self.docker("push", f"{self.repo}:{tag}")
            digests[tag] = aws("ecr", "describe-images", "--repository-name", self.prefix, "--image-ids", f"imageTag={tag}")["imageDetails"][0]["imageDigest"]
        self.images = {tag: f"{self.repo}@{digest}" for tag, digest in digests.items()}

    def make_fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), self.prefix, self.install, self.images["a"], self.images["b"]],
                       check=True, capture_output=True)
        self.fixtures = {path.stem: json.loads(path.read_text()) for path in out.glob("*.json")}
        (self.evidence / "fixtures").mkdir(exist_ok=True)
        for path in out.glob("*.json"):
            (self.evidence / "fixtures" / path.name).write_text(path.read_text())

    def verifier_policy(self, read_everything):
        """The verifier's permissions. `read_everything` widens the settings
        read to every parameter, as a careless setup might."""
        parameters = f"arn:aws:ssm:{REGION}:{self.account}:parameter"
        read = [f"{parameters}/signed/{self.prefix}/*"] if not read_everything else ["*"]
        return {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": ["lambda:GetFunction", "lambda:GetFunctionConfiguration", "lambda:UpdateFunctionCode", "lambda:UpdateFunctionConfiguration"],
             "Resource": f"arn:aws:lambda:{REGION}:{self.account}:function:{self.prefix}-*"},
            {"Effect": "Allow", "Action": "iam:PassRole", "Resource": f"arn:aws:iam::{self.account}:role/{self.exec_role}"},
            {"Effect": "Allow", "Action": "ssm:GetParameter", "Resource": read},
            {"Effect": "Allow", "Action": "ssm:PutParameter", "Resource": f"{parameters}{self.ledger_parameter}"},
            {"Effect": "Allow", "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer", "ecr:GetRepositoryPolicy", "ecr:SetRepositoryPolicy"],
             "Resource": f"arn:aws:ecr:{REGION}:{self.account}:repository/{self.prefix}"},
        ]}

    def set_verifier_policy(self, read_everything):
        aws("iam", "put-role-policy", "--role-name", self.verifier_role, "--policy-name", "verifier",
            "--policy-document", json.dumps(self.verifier_policy(read_everything)))
        time.sleep(15)  # IAM propagation

    def create_roles(self):
        trust = json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]})
        for role in (self.exec_role, self.verifier_role):
            aws("iam", "create-role", "--role-name", role, "--assume-role-policy-document", trust, "--tags", self.tags_list())
            aws("iam", "attach-role-policy", "--role-name", role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole")
        self.exec_role_arn = f"arn:aws:iam::{self.account}:role/{self.exec_role}"
        self.set_verifier_policy(read_everything=False)

    def write_setup_parameters(self):
        for name, value in [(self.settings_parameter, self.fixtures["settings"]), (self.ledger_parameter, self.fixtures["ledger"])]:
            aws("ssm", "put-parameter", "--name", name, "--type", "String", "--value", json.dumps(value),
                "--tags", f"Key={self.tag_key},Value={self.tag_value}")

    def wait_active(self, function):
        for _ in range(60):
            configuration = aws("lambda", "get-function-configuration", "--function-name", function)
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

    def create_verifier(self):
        aws("lambda", "create-function", "--function-name", self.verifier, "--runtime", "provided.al2023",
            "--handler", "bootstrap", "--architectures", "x86_64", "--zip-file", f"fileb://{self.zip}",
            "--role", f"arn:aws:iam::{self.account}:role/{self.verifier_role}", "--memory-size", "256", "--timeout", "300",
            "--environment", json.dumps({"Variables": {"VERIFIER_CLOUD": "aws", "RUST_LOG": "info"}}),
            "--tags", f"{self.tag_key}={self.tag_value}")
        self.wait_active(self.verifier)

    def create_functions(self):
        aws("lambda", "create-function", "--function-name", self.workload, "--package-type", "Image",
            "--code", f"ImageUri={self.images['a']}", "--role", self.exec_role_arn, "--architectures", "x86_64",
            "--memory-size", "128", "--timeout", "3", "--tags", f"{self.tag_key}={self.tag_value}")
        self.zip = self.verifier_zip()
        self.create_verifier()
        self.wait_active(self.workload)

    # --- calls ------------------------------------------------------------------

    def invoke(self, payload):
        out = self.work / f"out-{uuid.uuid4().hex[:6]}.json"
        response = aws("lambda", "invoke", "--function-name", self.verifier, "--cli-binary-format", "raw-in-base64-out",
                       "--payload", json.dumps(payload), str(out))
        body = json.loads(out.read_text() or "null")
        ok = not response.get("FunctionError")
        self.calls.append({"request": payload, "ok": ok, "response": body})
        if not ok:
            return False, body.get("errorMessage", "") if isinstance(body, dict) else str(body)
        return True, body

    def request(self, release, action="applyRelease", config="current", env=None):
        body = {"release": self.fixtures[release], "signerLists": [self.fixtures["signer-list"]],
                "providedEnv": {"api": {"ALIEN_DEPLOYMENT_ID": "dep_e2e", **(env or {})}}}
        config = self.config if config == "current" else config
        if config:
            body["config"] = self.fixtures[config]
        return {action: body}

    def function(self):
        return aws("lambda", "get-function", "--function-name", self.workload)

    def env(self):
        return self.function()["Configuration"].get("Environment", {}).get("Variables", {})

    def ledger(self):
        return json.loads(aws("ssm", "get-parameter", "--name", self.ledger_parameter)["Parameter"]["Value"])

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": ok, "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def expect_applied(self, name, release, image, expect_env=None, **request):
        ok, body = self.invoke(self.request(release, **request))
        function = self.function()
        env = function["Configuration"].get("Environment", {}).get("Variables", {})
        mismatched = {key: env.get(key) for key, value in (expect_env or {}).items() if not same_value(env.get(key), value)}
        good = (ok and env.get("WORKLOAD_MARKER") == release and function["Code"]["ImageUri"] == self.images[image]
                and function["Configuration"]["MemorySize"] == 256 and not mismatched)
        self.record(name, good, f"verifier ok={ok} marker={env.get('WORKLOAD_MARKER')} image={function['Code']['ImageUri'].rsplit('@', 1)[-1][:19]} mismatched={mismatched} body={str(body)[:300]}")
        return env

    def expect_refused(self, name, payload, reason, marker):
        ok, body = self.invoke(payload)
        still = self.env().get("WORKLOAD_MARKER")
        good = not ok and reason in str(body) and still == marker
        self.record(name, good, f"verifier ok={ok} still={still} reason={str(body)[:400]}")

    def change_directly(self, name, variables=None, logging=None):
        """What management of an unprotected install can do: write the
        function itself. Returns whether the change landed."""
        if variables is not None:
            aws("lambda", "update-function-configuration", "--function-name", self.workload,
                "--environment", json.dumps({"Variables": variables}))
        if logging is not None:
            aws("lambda", "update-function-configuration", "--function-name", self.workload,
                "--logging-config", json.dumps(logging))
        self.wait_active(self.workload)
        configuration = self.function()["Configuration"]
        landed = (variables is None or configuration.get("Environment", {}).get("Variables") == variables) and \
                 (logging is None or configuration.get("LoggingConfig", {}).get("LogGroup") == logging.get("LogGroup"))
        self.record(f"{name} (off: written straight to Lambda)", landed, f"landed={landed}")
        return landed

    def caught_and_undone(self, name, release, needle):
        ok, body = self.invoke(self.request(release, action="checkRelease"))
        self.record(f"{name} (on: checkRelease catches it)", not ok and needle in str(body), f"verifier ok={ok} reason={str(body)[:400]}")
        ok, body = self.invoke(self.request(release))
        ok_check, check_body = self.invoke(self.request(release, action="checkRelease"))
        self.record(f"{name} (on: applyRelease undoes it)", ok and ok_check, f"apply ok={ok} check ok={ok_check} {str(check_body)[:200]}")

    def run(self):
        self.push_images()
        self.make_fixtures()
        self.create_roles()
        self.write_setup_parameters()
        self.create_functions()
        log(f"ready: {self.workload}, {self.verifier}")

        derived = json.dumps({"service": "s3", "bucketName": f"{self.prefix}-data"}, separators=(",", ":"))

        # 1. The signed release with signed config lands as signed.
        self.config = "config-1"
        release_10 = self.fixtures["release-10"]
        env = self.expect_applied("1 signed release with signed config lands", "release-10", "a", {
            "LOG_LEVEL": "info", "ALIEN_DEPLOYMENT_ID": "dep_e2e", "ALIEN_DATA_BINDING": derived,
        })
        secrets = json.loads(env.get("WORKLOAD_SECRETS", "{}"))
        self.record("1 the secrets list is the signed one, in WORKLOAD_SECRETS", secrets.get("keys") == ["DB_PASSWORD"] and "ALIEN_SECRETS" not in env,
                    f"WORKLOAD_SECRETS={env.get('WORKLOAD_SECRETS')} ALIEN_SECRETS={env.get('ALIEN_SECRETS')}")
        ledger = self.ledger()
        self.record("1 the ledger rose to release 10 and config 1",
                    ledger.get("release", {}).get("sequence") == 10 and ledger.get("config", {}).get("sequence") == 1
                    and ledger.get("fingerprint") == self.fixtures["settings"]["fingerprint"] and ledger.get("install") == self.install,
                    json.dumps(ledger))

        # 2. Unsigned.
        self.expect_refused("2 unsigned release (on)", self.request("release-50-unsigned"), "the release was refused", "release-10")

        # 3. Binding redirect.
        attacker = json.dumps({"service": "s3", "bucketName": "attacker-bucket"})
        self.expect_refused("3 binding redirect from the control plane (on)",
                            self.request("release-10", env={"ALIEN_DATA_BINDING": attacker}), "don't fit the release", "release-10")
        self.config = "config-2-binding"
        self.expect_applied("3 signed config naming a derived binding: config lands, the derived value stays", "release-10", "a", {
            "LOG_LEVEL": "warn", "ALIEN_DATA_BINDING": derived,
        })
        variables = dict(self.env())
        variables["ALIEN_DATA_BINDING"] = attacker
        self.change_directly("3 binding redirect", variables=variables)
        self.caught_and_undone("3 binding redirect", "release-10", "ALIEN_DATA_BINDING")

        # 4. LD_PRELOAD from a vault.
        self.expect_refused("4 release whose secret is LD_PRELOAD (on)", self.request("release-60-ld-preload"),
                            "LD_PRELOAD changes how code loads", "release-10")
        self.expect_refused("4 secrets list naming LD_PRELOAD from the control plane (on)",
                            self.request("release-10", env={"WORKLOAD_SECRETS": json.dumps({"keys": ["LD_PRELOAD"], "hash": "x"})}),
                            "don't fit the release", "release-10")
        self.expect_refused("4 LD_PRELOAD from the control plane (on)",
                            self.request("release-10", env={"LD_PRELOAD": "/tmp/x.so"}), "don't fit the release", "release-10")
        variables = dict(self.env())
        variables["LD_PRELOAD"] = "/tmp/x.so"
        self.change_directly("4 LD_PRELOAD", variables=variables)
        self.caught_and_undone("4 LD_PRELOAD", "release-10", "LD_PRELOAD")

        # 5, 6. Wrong channel, wrong key.
        self.expect_refused("5 dev release to a production install (on)", self.request("release-30-dev"), "channel", "release-10")
        self.expect_refused("6 release signed by a key not on the list (on)", self.request("release-40-attacker"), "the release was refused", "release-10")

        # 7. Newer lands; a fresh verifier reads the ledger and refuses the older.
        self.expect_applied("7 newer release lands", "release-20", "b", {"LOG_LEVEL": "warn"})
        aws("lambda", "delete-function", "--function-name", self.verifier)
        self.create_verifier()
        self.expect_refused("7 fresh verifier refuses the older release (on)", self.request("release-10"),
                            "older than one this install accepted", "release-20")
        self.expect_applied("7 fresh verifier accepts the release at the floor again", "release-20", "b")
        landed = aws("lambda", "update-function-code", "--function-name", self.workload, "--image-uri", self.images["a"], check=False, parse=False).returncode == 0
        self.wait_active(self.workload)
        self.record("7 older image (off: written straight to Lambda)", landed and self.function()["Code"]["ImageUri"] == self.images["a"], f"landed={landed}")
        self.caught_and_undone("7 older image", "release-20", "runs image")

        # 8. A log destination changed behind the verifier (D24 read-back).
        self.change_directly("8 log destination", logging={"LogFormat": "Text", "LogGroup": f"/{self.prefix}/elsewhere"})
        self.caught_and_undone("8 log destination", "release-20", "its logging sends LogGroup")

        # 9. Sticky: the settings disappear after acceptance.
        aws("ssm", "delete-parameter", "--name", self.settings_parameter)
        self.expect_refused("9 settings deleted after acceptance (on)", self.request("release-20"),
                            "are gone, and this install accepted signed state", "release-20")
        aws("ssm", "put-parameter", "--name", self.settings_parameter, "--type", "String", "--value", json.dumps(self.fixtures["settings"]),
            "--tags", f"Key={self.tag_key},Value={self.tag_value}")
        self.expect_applied("9 settings restored: accepted again", "release-20", "b")

        # 10. Two candidate settings.
        decoy = dict(self.fixtures["settings"], prefix=self.verifier)
        aws("ssm", "put-parameter", "--name", self.decoy_parameter, "--type", "String", "--value", json.dumps(decoy),
            "--tags", f"Key={self.tag_key},Value={self.tag_value}")
        self.expect_applied("10 decoy settings unreadable to a scoped verifier (off: accepted)", "release-20", "b")
        self.set_verifier_policy(read_everything=True)
        self.expect_refused("10 verifier that reads every parameter sees two candidates (on)", self.request("release-20"),
                            "2 places hold signing settings", "release-20")

    # --- teardown ---------------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        for function in (self.verifier, self.workload):
            aws("lambda", "delete-function", "--function-name", function, check=False, parse=False)
            aws("logs", "delete-log-group", "--log-group-name", f"/aws/lambda/{function}", check=False, parse=False)
        aws("logs", "delete-log-group", "--log-group-name", f"/{self.prefix}/elsewhere", check=False, parse=False)
        for parameter in (self.settings_parameter, self.ledger_parameter, self.decoy_parameter):
            aws("ssm", "delete-parameter", "--name", parameter, check=False, parse=False)
        aws("iam", "delete-role-policy", "--role-name", self.verifier_role, "--policy-name", "verifier", check=False, parse=False)
        for role in (self.exec_role, self.verifier_role):
            aws("iam", "detach-role-policy", "--role-name", role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole", check=False, parse=False)
            aws("iam", "delete-role", "--role-name", role, check=False, parse=False)
        aws("ecr", "delete-repository", "--repository-name", self.prefix, "--force", check=False, parse=False)

    def audit(self):
        """What of this run is still there."""
        left = []
        for function in (self.verifier, self.workload):
            if aws("lambda", "get-function", "--function-name", function, check=False, parse=False).returncode == 0:
                left.append(f"function {function}")
        for role in (self.exec_role, self.verifier_role):
            if aws("iam", "get-role", "--role-name", role, check=False, parse=False).returncode == 0:
                left.append(f"role {role}")
        for parameter in (self.settings_parameter, self.ledger_parameter, self.decoy_parameter):
            if aws("ssm", "get-parameter", "--name", parameter, check=False, parse=False).returncode == 0:
                left.append(f"parameter {parameter}")
        if aws("ecr", "describe-repositories", "--repository-names", self.prefix, check=False, parse=False).returncode == 0:
            left.append(f"repository {self.prefix}")
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--verifier", required=True, help="alien-gate binary for x86_64 Linux (musl)")
    parser.add_argument("--fixtures", required=True, help="the gate-fixtures example binary")
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
