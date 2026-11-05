#!/usr/bin/env python3
"""End-to-end test of the gate on real GCP (Cloud Run).

Creates, all named with --prefix and labeled with --label:
  - an Artifact Registry repository with two sample images and the gate's image;
  - service accounts for the gate, the workload and a stand-in for the
    management identity;
  - the workload service `<prefix>-fn-api`, as setup would create it;
  - the gate service, configured with a fresh anchor;
  - the gate's state secret and the customer's deployer inputs secret.

The management stand-in may only call the gate and read the workload. It
calls the gate the way the control plane would, and each result is checked
by reading the service back with the caller's own credentials:
  1. a signed release is applied exactly as signed;
  2. a spec passed by the control plane is refused (unknown field);
  3. env values the release doesn't allow are refused;
  4. a newer release is applied; the older one replayed is refused;
  5. a release signed by a key not on the list is refused;
  6. management can't update the workload service directly;
  7. a change made behind the gate is caught by `check` and undone by `apply`;
  8. a deployer input comes from the customer's secret, never from the
     control plane.

Usage (an access token for the test project in --token-file):
    scripts/gate-e2e/gcp.py --gate target/release/alien-gate \\
        --fixtures target/debug/examples/gate-fixtures \\
        --project my-test-project --region us-central1 --token-file /path/to/token \\
        --prefix test-gate --label purpose=e2e --evidence /tmp/gate-e2e-gcp
"""

import argparse
import base64
import json
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class Run:
    def __init__(self, args):
        self.args = args
        self.suffix = uuid.uuid4().hex[:6]
        self.name = f"{args.prefix}-{self.suffix}"
        self.project, self.region = args.project, args.region
        self.label = args.label
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="gate-e2e-gcp-"))
        self.registry = f"{self.region}-docker.pkg.dev/{self.project}/{self.name}"
        self.workload_service = f"{self.name}-fn-api"
        self.gate_service = f"{self.name}-gate"
        self.state_secret = f"{self.name}-state"
        self.inputs_secret = f"{self.name}-inputs"
        self.accounts = {role: f"{self.name}-{role}@{self.project}.iam.gserviceaccount.com" for role in ("gate", "exec", "mgmt")}
        self.results = []

    # --- helpers -------------------------------------------------------------

    def gcloud(self, *args, check=True, input=None):
        result = subprocess.run(["gcloud", *args, "--project", self.project, "--access-token-file", self.args.token_file, "--quiet"],
                                capture_output=True, text=True, input=input)
        if check and result.returncode != 0:
            raise RuntimeError(f"gcloud {' '.join(args[:3])} failed: {result.stderr.strip()[-800:]}")
        return result

    def token(self):
        return pathlib.Path(self.args.token_file).read_text().strip()

    def http(self, method, url, body=None, token=None):
        request = urllib.request.Request(url, method=method, data=None if body is None else json.dumps(body).encode())
        request.add_header("Authorization", f"Bearer {token or self.token()}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=600) as response:
                text = response.read().decode()
                return response.status, json.loads(text) if text.strip() else None
        except urllib.error.HTTPError as error:
            text = error.read().decode()
            try:
                return error.code, json.loads(text)
            except json.JSONDecodeError:
                return error.code, text

    def service_url(self, service):
        return f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services/{service}"

    def docker(self, *args, input=None):
        subprocess.run(["docker", *args], check=True, capture_output=True, input=input)

    def digest(self, image):
        out = subprocess.run(["docker", "inspect", "--format", "{{index .RepoDigests 0}}", image], check=True, capture_output=True, text=True).stdout.strip()
        return out.rsplit("@", 1)[1]

    # --- setup ---------------------------------------------------------------

    def push_images(self):
        key, value = self.label.split("=", 1)
        self.gcloud("artifacts", "repositories", "create", self.name, "--repository-format=docker", f"--location={self.region}", f"--labels={key}={value}")
        self.docker("login", "-u", "oauth2accesstoken", "--password-stdin", f"https://{self.region}-docker.pkg.dev", input=self.token().encode())
        self.images = {}
        for tag, source in [("a", "gcr.io/google-samples/hello-app:1.0"), ("b", "gcr.io/google-samples/hello-app:2.0")]:
            self.docker("pull", "--platform", "linux/amd64", source)
            self.docker("tag", source, f"{self.registry}/app:{tag}")
            self.docker("push", f"{self.registry}/app:{tag}")
            self.images[tag] = f"{self.registry}/app@{self.digest(f'{self.registry}/app:{tag}')}"
        build = self.work / "gate-image"
        build.mkdir()
        (build / "alien-gate").write_bytes(pathlib.Path(self.args.gate).read_bytes())
        (build / "Dockerfile").write_text(
            "FROM public.ecr.aws/docker/library/debian:bookworm-slim\n"
            "RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*\n"
            "COPY alien-gate /alien-gate\nRUN chmod +x /alien-gate\nENTRYPOINT [\"/alien-gate\"]\n"
        )
        self.docker("build", "--platform", "linux/amd64", "-t", f"{self.registry}/gate:latest", str(build))
        self.docker("push", f"{self.registry}/gate:latest")
        self.images["gate"] = f"{self.registry}/gate@{self.digest(f'{self.registry}/gate:latest')}"

    def create_accounts_and_secrets(self):
        for role in self.accounts:
            self.gcloud("iam", "service-accounts", "create", f"{self.name}-{role}", f"--display-name={self.name} {role}")
        time.sleep(10)
        key, value = self.label.split("=", 1)
        for secret in (self.state_secret, self.inputs_secret):
            self.gcloud("secrets", "create", secret, "--replication-policy=automatic", f"--labels={key}={value}")
        for role in ("roles/secretmanager.secretAccessor", "roles/secretmanager.secretVersionAdder"):
            self.gcloud("secrets", "add-iam-policy-binding", self.state_secret, f"--member=serviceAccount:{self.accounts['gate']}", f"--role={role}")
        self.gcloud("secrets", "add-iam-policy-binding", self.inputs_secret, f"--member=serviceAccount:{self.accounts['gate']}", "--role=roles/secretmanager.secretAccessor")
        # The gate may run workloads as the workload's account, and only that one.
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.accounts["exec"], f"--member=serviceAccount:{self.accounts['gate']}", "--role=roles/iam.serviceAccountUser")
        # The caller mints the management stand-in's identity tokens.
        status, me = self.http("GET", "https://www.googleapis.com/oauth2/v3/userinfo")
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.accounts["mgmt"], f"--member=user:{me['email']}", "--role=roles/iam.serviceAccountTokenCreator")
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.accounts["mgmt"], f"--member=user:{me['email']}", "--role=roles/iam.serviceAccountTokenCreator")

    def make_fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), "shop", "acme", f"gcp-secret:{self.inputs_secret}", self.images["a"], self.images["b"]],
                       check=True, capture_output=True)
        self.fixtures = {path.stem: json.loads(path.read_text()) for path in out.glob("*.json")}

    def create_services(self):
        key, value = self.label.split("=", 1)
        self.gcloud("run", "deploy", self.workload_service, f"--image={self.images['a']}", f"--region={self.region}",
                    f"--service-account={self.accounts['exec']}", "--no-allow-unauthenticated", "--memory=128Mi", f"--labels={key}={value}")
        self.gcloud("run", "services", "add-iam-policy-binding", self.workload_service, f"--region={self.region}",
                    f"--member=serviceAccount:{self.accounts['gate']}", "--role=roles/run.developer")
        self.gcloud("run", "services", "add-iam-policy-binding", self.workload_service, f"--region={self.region}",
                    f"--member=serviceAccount:{self.accounts['mgmt']}", "--role=roles/run.viewer")
        env = {
            "ALIEN_GATE_CLOUD": "gcp",
            "ALIEN_GATE_ANCHOR": json.dumps(self.fixtures["anchor"]),
            "ALIEN_GATE_FUNCTION_PREFIX": f"{self.name}-fn-",
            "ALIEN_GATE_REGISTRY": f"{self.registry}/",
            "ALIEN_GATE_ROLES": json.dumps({"execution": self.accounts["exec"]}),
            "ALIEN_GATE_STATE_SECRET": self.state_secret,
            "ALIEN_GATE_GCP_PROJECT": self.project,
            "ALIEN_GATE_GCP_REGION": self.region,
        }
        env_file = self.work / "gate-env.json"
        env_file.write_text(json.dumps(env))
        self.gcloud("run", "deploy", self.gate_service, f"--image={self.images['gate']}", f"--region={self.region}",
                    f"--service-account={self.accounts['gate']}", "--no-allow-unauthenticated", "--memory=512Mi", "--timeout=600",
                    f"--env-vars-file={env_file}", f"--labels={key}={value}")
        self.gcloud("run", "services", "add-iam-policy-binding", self.gate_service, f"--region={self.region}",
                    f"--member=serviceAccount:{self.accounts['mgmt']}", "--role=roles/run.invoker")
        self.gate_url = self.http("GET", self.service_url(self.gate_service))[1]["uri"]

    # --- the management stand-in -------------------------------------------------

    def mgmt_token(self, audience=None):
        account = self.accounts["mgmt"]
        if audience:
            url = f"https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{account}:generateIdToken"
            status, body = self.http("POST", url, {"audience": audience, "includeEmail": True})
        else:
            url = f"https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{account}:generateAccessToken"
            status, body = self.http("POST", url, {"scope": ["https://www.googleapis.com/auth/cloud-platform"]})
        if status != 200:
            raise RuntimeError(f"minting a token for {account}: {status} {body}")
        return body.get("token") or body["accessToken"]

    def invoke(self, payload):
        for attempt in range(6):
            status, body = self.http("POST", self.gate_url, payload, token=self.mgmt_token(self.gate_url))
            if status not in (401, 403) or attempt == 5 or isinstance(body, dict):
                break
            time.sleep(10)  # IAM propagation for the invoker binding
        if status == 200:
            return True, body
        return False, body.get("errorMessage", str(body)) if isinstance(body, dict) else str(body)

    def request(self, release, action="apply", **extra):
        payload = {"action": action, "workload": "api", "signedRelease": self.fixtures[release],
                   "signerLists": [self.fixtures["signer-list"]], "env": {"ALIEN_DEPLOYMENT_ID": "acme"}}
        payload.update(extra)
        return payload

    def service(self):
        return self.http("GET", self.service_url(self.workload_service))[1]

    def env(self):
        container = self.service()["template"]["containers"][0]
        return {item["name"]: item.get("value") for item in container.get("env", [])}

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": ok, "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def expect_applied(self, name, release, image):
        ok, body = self.invoke(self.request(release))
        container = self.service()["template"]["containers"][0]
        marker = {item["name"]: item.get("value") for item in container.get("env", [])}.get("WORKLOAD_MARKER")
        good = ok and marker == release and container["image"] == self.images[image]
        self.record(name, good, f"gate ok={ok} marker={marker} image={container['image'].rsplit('@', 1)[-1][:19]} body={str(body)[:200]}")

    def expect_refused(self, name, payload, reason, marker):
        ok, body = self.invoke(payload)
        still = self.env().get("WORKLOAD_MARKER")
        good = not ok and reason in str(body) and still == marker
        self.record(name, good, f"gate ok={ok} still={still} reason={str(body)[:300]}")

    def wait_ready(self):
        for _ in range(100):
            if not self.service().get("reconciling"):
                return
            time.sleep(3)

    def run(self):
        self.push_images()
        self.create_accounts_and_secrets()
        self.make_fixtures()
        self.create_services()
        log(f"services ready: {self.workload_service}, {self.gate_service}")

        self.expect_applied("signed release applied", "release-10", "a")
        spec = self.request("release-10", template={"containers": [{"image": "evil"}]})
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

        # The lock: management may call the gate and read, not write the service.
        body = self.service()
        body["template"]["containers"][0]["env"].append({"name": "NODE_OPTIONS", "value": "--require /tmp/ATTACK.js"})
        status, response = self.http("PATCH", self.service_url(self.workload_service), {"template": body["template"]}, token=self.mgmt_token())
        self.record("management can't update the service directly", status == 403 and "NODE_OPTIONS" not in self.env(),
                    f"HTTP {status} {str(response)[:200]}")

        # A change behind the gate, as an identity without the lock could make.
        status, _ = self.http("PATCH", self.service_url(self.workload_service), {"template": body["template"]})
        self.wait_ready()
        ok, reason = self.invoke(self.request("release-20", action="check"))
        self.record("check catches a change behind the gate", status == 200 and not ok and "NODE_OPTIONS" in str(reason),
                    f"owner patch HTTP {status}; gate ok={ok} reason={str(reason)[:300]}")
        ok, _ = self.invoke(self.request("release-20"))
        ok_check, body = self.invoke(self.request("release-20", action="check"))
        self.record("apply undoes it", ok and ok_check and "NODE_OPTIONS" not in self.env(), f"apply ok={ok} check ok={ok_check}")

        # Deployer inputs: the customer's secret wins over the control plane.
        self.gcloud("secrets", "versions", "add", self.inputs_secret, "--data-file=-", input=json.dumps({"region": "customer-eu"}))
        payload = self.request("release-40-deployer-input")
        payload["env"]["DEPLOYER_REGION"] = "ATTACK-control-plane"
        ok, body = self.invoke(payload)
        region = self.env().get("DEPLOYER_REGION")
        self.record("deployer input from the customer's store", ok and region == "customer-eu", f"gate ok={ok} DEPLOYER_REGION={region} body={str(body)[:200]}")

    # --- teardown ---------------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        for service in (self.gate_service, self.workload_service):
            self.gcloud("run", "services", "delete", service, f"--region={self.region}", check=False)
        for secret in (self.state_secret, self.inputs_secret):
            self.gcloud("secrets", "delete", secret, check=False)
        for account in self.accounts.values():
            self.gcloud("iam", "service-accounts", "delete", account, check=False)
        self.gcloud("artifacts", "repositories", "delete", self.name, f"--location={self.region}", check=False)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--gate", required=True, help="alien-gate binary for linux/amd64")
    parser.add_argument("--fixtures", required=True, help="the gate-fixtures example binary")
    parser.add_argument("--project", required=True)
    parser.add_argument("--region", default="us-central1")
    parser.add_argument("--token-file", required=True, help="file holding an access token for the project")
    parser.add_argument("--prefix", required=True, help="prefix for every resource name")
    parser.add_argument("--label", required=True, help="key=value label for every resource that takes labels")
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
