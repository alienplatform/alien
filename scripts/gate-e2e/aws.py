#!/usr/bin/env python3
"""End-to-end test of the gate on real AWS.

Creates, all named with --prefix and tagged with --tag:
  - an ECR repository with two small images and the gate's image;
  - an execution role for the workload and a role for the gate;
  - the workload function `<prefix>-fn-api`, as setup would create it;
  - the gate function, configured with a fresh anchor;
  - the gate's state parameter and the customer's deployer inputs parameter.

Then calls the gate the way the control plane would and checks each result
by reading the function back with the caller's own credentials:
  1. a signed release is applied exactly as signed;
  2. a spec passed by the control plane is refused (unknown field);
  3. an env value the release doesn't allow is refused;
  4. a newer release is applied; the older one replayed is refused;
  5. a release signed by a key not on the list is refused;
  6. a change made behind the gate is caught by `check` and undone by `apply`;
  7. a deployer input comes from the customer's parameter, set with
     `alien-deploy inputs set`, and never from the control plane.

Usage (credentials for a test account in the environment):
    scripts/gate-e2e/aws.py --gate target/release/alien-gate \\
        --fixtures target/debug/examples/gate-fixtures \\
        --alien-deploy target/debug/alien-deploy \\
        --prefix test-gate --tag purpose=e2e --evidence /tmp/gate-e2e
"""

import argparse
import base64
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import uuid

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


class Run:
    def __init__(self, args):
        self.args = args
        self.suffix = uuid.uuid4().hex[:6]
        self.name = f"{args.prefix}-{self.suffix}"
        key, value = args.tag.split("=", 1)
        self.tag_key, self.tag_value = key, value
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="gate-e2e-"))
        self.account = aws("sts", "get-caller-identity")["Account"]
        self.registry = f"{self.account}.dkr.ecr.{REGION}.amazonaws.com"
        self.repo = f"{self.registry}/{self.name}"
        self.workload_function = f"{self.name}-fn-api"
        self.gate_function = f"{self.name}-gate"
        self.state_parameter = f"/{self.name}/gate-state"
        self.inputs_parameter = f"/{self.name}/deployer-inputs"
        self.exec_role = f"{self.name}-exec"
        self.gate_role = f"{self.name}-gate"
        self.results = []

    def tags_list(self):
        return json.dumps([{"Key": self.tag_key, "Value": self.tag_value}])

    # --- setup --------------------------------------------------------------

    def docker(self, *args, input=None):
        subprocess.run(["docker", *args], check=True, capture_output=True, input=input)

    def push_images(self):
        aws("ecr", "create-repository", "--repository-name", self.name, "--tags", self.tags_list())
        password = aws("ecr", "get-login-password", parse=False).stdout
        self.docker("login", "--username", "AWS", "--password-stdin", self.registry, input=password.encode())
        digests = {}
        for tag, source in [("a", "public.ecr.aws/docker/library/busybox:1.36"), ("b", "public.ecr.aws/docker/library/alpine:3.20")]:
            self.docker("pull", "--platform", "linux/amd64", source)
            self.docker("tag", source, f"{self.repo}:{tag}")
            self.docker("push", f"{self.repo}:{tag}")
            digests[tag] = aws("ecr", "describe-images", "--repository-name", self.name, "--image-ids", f"imageTag={tag}")["imageDetails"][0]["imageDigest"]
        build = self.work / "gate-image"
        build.mkdir()
        (build / "alien-gate").write_bytes(pathlib.Path(self.args.gate).read_bytes())
        (build / "Dockerfile").write_text(
            "FROM public.ecr.aws/docker/library/debian:bookworm-slim\n"
            "RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*\n"
            "COPY alien-gate /alien-gate\nRUN chmod +x /alien-gate\nENTRYPOINT [\"/alien-gate\"]\n"
        )
        self.docker("build", "--platform", "linux/amd64", "-t", f"{self.repo}:gate", str(build))
        self.docker("push", f"{self.repo}:gate")
        digests["gate"] = aws("ecr", "describe-images", "--repository-name", self.name, "--image-ids", "imageTag=gate")["imageDetails"][0]["imageDigest"]
        self.images = {tag: f"{self.repo}@{digest}" for tag, digest in digests.items()}

    def create_roles(self):
        trust = json.dumps({"Version": "2012-10-17", "Statement": [{"Effect": "Allow", "Principal": {"Service": "lambda.amazonaws.com"}, "Action": "sts:AssumeRole"}]})
        for role in (self.exec_role, self.gate_role):
            aws("iam", "create-role", "--role-name", role, "--assume-role-policy-document", trust, "--tags", self.tags_list())
            aws("iam", "attach-role-policy", "--role-name", role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole")
        self.exec_role_arn = f"arn:aws:iam::{self.account}:role/{self.exec_role}"
        policy = {"Version": "2012-10-17", "Statement": [
            {"Effect": "Allow", "Action": ["lambda:GetFunction", "lambda:UpdateFunctionCode", "lambda:UpdateFunctionConfiguration"],
             "Resource": f"arn:aws:lambda:{REGION}:{self.account}:function:{self.name}-fn-*"},
            {"Effect": "Allow", "Action": "iam:PassRole", "Resource": self.exec_role_arn},
            {"Effect": "Allow", "Action": ["ssm:GetParameter", "ssm:PutParameter"], "Resource": f"arn:aws:ssm:{REGION}:{self.account}:parameter{self.state_parameter}"},
            {"Effect": "Allow", "Action": "ssm:GetParameter", "Resource": f"arn:aws:ssm:{REGION}:{self.account}:parameter{self.inputs_parameter}"},
            {"Effect": "Allow", "Action": ["ecr:BatchGetImage", "ecr:GetDownloadUrlForLayer", "ecr:GetRepositoryPolicy"], "Resource": f"arn:aws:ecr:{REGION}:{self.account}:repository/{self.name}"},
        ]}
        aws("iam", "put-role-policy", "--role-name", self.gate_role, "--policy-name", "gate", "--policy-document", json.dumps(policy))
        time.sleep(15)  # IAM propagation before Lambda can assume the roles

    def make_fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), "shop", "acme", f"aws-ssm:{self.inputs_parameter}", self.images["a"], self.images["b"]],
                       check=True, capture_output=True)
        self.fixtures = {path.stem: json.loads(path.read_text()) for path in out.glob("*.json")}

    def wait_active(self, function):
        for _ in range(60):
            configuration = aws("lambda", "get-function-configuration", "--function-name", function)
            if configuration.get("State") == "Active" and configuration.get("LastUpdateStatus") in (None, "Successful"):
                return
            time.sleep(5)
        raise RuntimeError(f"{function} did not become active")

    def create_functions(self):
        tags = f"{self.tag_key}={self.tag_value}"
        aws("lambda", "create-function", "--function-name", self.workload_function, "--package-type", "Image",
            "--code", f"ImageUri={self.images['a']}", "--role", self.exec_role_arn, "--memory-size", "128", "--timeout", "3", "--tags", tags)
        environment = {"Variables": {
            "ALIEN_GATE_ANCHOR": json.dumps(self.fixtures["anchor"]),
            "ALIEN_GATE_FUNCTION_PREFIX": f"{self.name}-fn-",
            "ALIEN_GATE_REGISTRY": f"{self.repo}",
            "ALIEN_GATE_ROLES": json.dumps({"execution": self.exec_role_arn}),
            "ALIEN_GATE_STATE_PARAMETER": self.state_parameter,
        }}
        aws("lambda", "create-function", "--function-name", self.gate_function, "--package-type", "Image",
            "--code", f"ImageUri={self.images['gate']}", "--role", f"arn:aws:iam::{self.account}:role/{self.gate_role}",
            "--memory-size", "256", "--timeout", "300", "--environment", json.dumps(environment), "--tags", tags)
        self.wait_active(self.workload_function)
        self.wait_active(self.gate_function)

    # --- calls ----------------------------------------------------------------

    def invoke(self, payload):
        out = self.work / f"out-{uuid.uuid4().hex[:6]}.json"
        response = aws("lambda", "invoke", "--function-name", self.gate_function, "--cli-binary-format", "raw-in-base64-out",
                       "--payload", json.dumps(payload), str(out))
        body = json.loads(out.read_text() or "null")
        if response.get("FunctionError"):
            return False, body.get("errorMessage", "") if isinstance(body, dict) else str(body)
        return True, body

    def request(self, release, action="apply", **extra):
        payload = {"action": action, "workload": "api", "signedRelease": self.fixtures[release],
                   "signerLists": [self.fixtures["signer-list"]], "env": {"ALIEN_DEPLOYMENT_ID": "acme"}}
        payload.update(extra)
        return payload

    def function(self):
        return aws("lambda", "get-function", "--function-name", self.workload_function)

    def env(self):
        return self.function()["Configuration"].get("Environment", {}).get("Variables", {})

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": ok, "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def expect_applied(self, name, release, image):
        ok, body = self.invoke(self.request(release))
        function = self.function()
        marker = function["Configuration"].get("Environment", {}).get("Variables", {}).get("WORKLOAD_MARKER")
        good = ok and marker == release and function["Code"]["ImageUri"] == self.images[image] and function["Configuration"]["MemorySize"] == 256
        self.record(name, good, f"gate ok={ok} marker={marker} image={function['Code']['ImageUri'].rsplit('@', 1)[-1][:19]} body={str(body)[:200]}")

    def expect_refused(self, name, payload, reason, marker):
        ok, body = self.invoke(payload)
        still = self.env().get("WORKLOAD_MARKER")
        good = not ok and reason in body and still == marker
        self.record(name, good, f"gate ok={ok} still={still} reason={body[:300] if isinstance(body, str) else body}")

    def run(self):
        self.push_images()
        self.create_roles()
        self.make_fixtures()
        self.create_functions()
        log(f"functions ready: {self.workload_function}, {self.gate_function}")

        self.expect_applied("signed release applied", "release-10", "a")
        spec = self.request("release-10", environment={"Variables": {"WORKLOAD_MARKER": "ATTACK"}})
        self.expect_refused("spec from the control plane", spec, "unknown field", "release-10")
        override = self.request("release-10")
        override["env"]["WORKLOAD_MARKER"] = "ATTACK"
        self.expect_refused("env the release doesn't allow", override, "don't fit the release", "release-10")
        loader = self.request("release-10")
        loader["env"]["NODE_OPTIONS"] = "--require /tmp/x.js"
        self.expect_refused("NODE_OPTIONS from the control plane", loader, "don't fit the release", "release-10")
        self.expect_applied("newer release applied", "release-20", "b")
        self.expect_refused("older release replayed", self.request("release-10"), "older than one already applied", "release-20")
        self.expect_refused("release signed by another key", self.request("release-30-attacker"), "release was refused", "release-20")
        other = self.request("release-20", image=f"{self.registry}/elsewhere@{self.images['b'].rsplit('@', 1)[1]}")
        self.expect_refused("image outside the registry", other, "outside this deployment's registry", "release-20")

        # A change behind the gate, as a management role without the locks could make.
        variables = dict(self.env())
        variables["NODE_OPTIONS"] = "--require /tmp/ATTACK.js"
        aws("lambda", "update-function-configuration", "--function-name", self.workload_function,
            "--environment", json.dumps({"Variables": variables}))
        self.wait_active(self.workload_function)
        ok, body = self.invoke(self.request("release-20", action="check"))
        self.record("check catches a change behind the gate", not ok and "NODE_OPTIONS" in body, f"gate ok={ok} reason={str(body)[:300]}")
        ok, _ = self.invoke(self.request("release-20"))
        ok_check, body = self.invoke(self.request("release-20", action="check"))
        self.record("apply undoes it", ok and ok_check and "NODE_OPTIONS" not in self.env(), f"apply ok={ok} check ok={ok_check}")

        # Deployer inputs: the customer's parameter wins over the control plane.
        subprocess.run([self.args.alien_deploy, "inputs", "set", "--store", f"aws-ssm:{self.inputs_parameter}", "--input", "region=customer-eu"],
                       check=True, capture_output=True)
        aws("ssm", "add-tags-to-resource", "--resource-type", "Parameter", "--resource-id", self.inputs_parameter, "--tags", f"Key={self.tag_key},Value={self.tag_value}", parse=False)
        payload = self.request("release-40-deployer-input")
        payload["env"]["DEPLOYER_REGION"] = "ATTACK-control-plane"
        ok, body = self.invoke(payload)
        region = self.env().get("DEPLOYER_REGION")
        self.record("deployer input from the customer's store", ok and region == "customer-eu", f"gate ok={ok} DEPLOYER_REGION={region} body={str(body)[:200]}")

    # --- teardown ---------------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        for function in (self.gate_function, self.workload_function):
            aws("lambda", "delete-function", "--function-name", function, check=False, parse=False)
            aws("logs", "delete-log-group", "--log-group-name", f"/aws/lambda/{function}", check=False, parse=False)
        for parameter in (self.state_parameter, self.inputs_parameter):
            aws("ssm", "delete-parameter", "--name", parameter, check=False, parse=False)
        aws("iam", "delete-role-policy", "--role-name", self.gate_role, "--policy-name", "gate", check=False, parse=False)
        for role in (self.exec_role, self.gate_role):
            aws("iam", "detach-role-policy", "--role-name", role, "--policy-arn", "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole", check=False, parse=False)
            aws("iam", "delete-role", "--role-name", role, check=False, parse=False)
        aws("ecr", "delete-repository", "--repository-name", self.name, "--force", check=False, parse=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--gate", required=True, help="alien-gate binary for linux/amd64")
    parser.add_argument("--fixtures", required=True, help="the gate-fixtures example binary")
    parser.add_argument("--alien-deploy", required=True)
    parser.add_argument("--prefix", required=True, help="prefix for every resource name")
    parser.add_argument("--tag", required=True, help="key=value tag for every resource")
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()
    run = Run(args)
    try:
        run.run()
    finally:
        (run.evidence / "results.json").write_text(json.dumps(run.results, indent=2))
        if not args.keep:
            run.cleanup()
    failed = [result for result in run.results if not result["pass"]]
    if failed or not run.results:
        sys.exit(1)


if __name__ == "__main__":
    main()
