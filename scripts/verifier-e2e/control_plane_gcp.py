#!/usr/bin/env python3
"""A protected install's Cloud Run worker, created and updated by the control plane on real GCP.

The GCP counterpart of `control_plane.py`. Sets up, as protected setup would, an install named
`<p>` in --project (all names start with `<p>` and carry --label):
  - an Artifact Registry repository with two worker images (HTTP on 8080) and the verifier's;
  - the workload's account `<p>-execution-sa`, the verifier's `<p>-verifier`, and a management
    stand-in `<p>-mgmt` that may read Cloud Run and call the verifier, nothing more;
  - the signing settings and ledger in Parameter Manager (`signed-<p>-fingerprint`,
    `signed-<p>-state`), the verifier allowed to list and read them (parameterViewer) and to add
    ledger versions;
  - the verifier as Cloud Run service `<p>-verifier`.
No setup creates the worker's service. As the management stand-in (the caller impersonates it),
`protected_worker_update update` (alien-infra example, CLOUD=gcp) runs the real stack executor and
GCP worker controller, which call the verifier with an ID token, and the script checks with the
caller's credentials:
  0. the first deployment creates `<p>-api` through the verifier: the signed image, internal
     ingress (the worker has no public endpoint), and the worker's account may invoke it;
  1. an update moves it to the second image through the verifier, and management's own
     run.services.update is denied;
  2. a release under a signer list setup didn't pin is refused with the verifier's reason, and the
     service keeps the second image.
Everything is deleted at the end and the run fails if anything named `<p>` is left.

    scripts/verifier-e2e/control_plane_gcp.py \\
        --verifier target/x86_64-unknown-linux-musl/debug/alien-verifier \\
        --driver target/debug/examples/protected_worker_update \\
        --project my-test-project --region us-central1 --token-file /path/to/token \\
        --prefix e2e-cpg --label purpose=e2e --evidence /tmp/control-plane-gcp

Requires gcloud, crane and Docker, and an access token for a disposable test project.
"""

import argparse
import datetime
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import uuid


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class Run:
    def __init__(self, args):
        self.args = args
        self.prefix = f"{args.prefix}{uuid.uuid4().hex[:4]}"
        self.label_key, self.label_value = args.label.split("=", 1)
        self.project, self.region = args.project, args.region
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="control-plane-gcp-"))
        self.registry_host = f"{self.region}-docker.pkg.dev"
        self.repo = f"{self.registry_host}/{self.project}/{self.prefix}"
        self.not_after = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=30)) \
            .replace(microsecond=0).isoformat()
        self.sa = {name: f"{self.prefix}-{name}@{self.project}.iam.gserviceaccount.com"
                   for name in ("verifier", "execution-sa", "mgmt")}
        self.results = []
        self.bindings = []
        self.created = []

    # --- helpers ----------------------------------------------------------------

    def gcloud(self, *args, check=True, token_file=None):
        command = ["gcloud", *args, "--project", self.project, "--quiet", "--format=json",
                   f"--access-token-file={token_file or self.args.token_file}"]
        result = subprocess.run(command, capture_output=True, text=True)
        if check and result.returncode != 0:
            raise RuntimeError(f"gcloud {' '.join(args[:3])} failed: {result.stderr.strip()[-1500:]}")
        return result

    def gcloud_json(self, *args, **kwargs):
        out = self.gcloud(*args, **kwargs).stdout
        return json.loads(out) if out.strip() else {}

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def token(self):
        return pathlib.Path(self.args.token_file).read_text().strip()

    def management_token_file(self):
        """An access token for the management stand-in, minted with the caller's token."""
        url = ("https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/"
               f"{self.sa['mgmt']}:generateAccessToken")
        body = json.dumps({"scope": ["https://www.googleapis.com/auth/cloud-platform"]})
        response = subprocess.run(["curl", "-s", "-X", "POST", "-H", f"Authorization: Bearer {self.token()}",
                                   "-H", "Content-Type: application/json", "-d", body, url],
                                  capture_output=True, text=True, check=True).stdout
        path = self.work / "management-token"
        path.write_text(json.loads(response)["accessToken"])
        return str(path)

    def driver(self, command, env=None, check=True):
        variables = {**os.environ, "PREFIX": self.prefix, "NOT_AFTER": self.not_after, "CLOUD": "gcp",
                     "GCP_PROJECT": self.project, "GCP_REGION": self.region,
                     "GCP_PROJECT_NUMBER": self.project_number, "GCP_TOKEN_FILE": self.args.token_file,
                     "MANAGEMENT_SA": self.sa["mgmt"], "EXECUTION_SA_UNIQUE_ID": getattr(self, "exec_id", ""),
                     **(env or {})}
        result = subprocess.run([self.args.driver, command], capture_output=True, text=True, env=variables)
        (self.evidence / f"driver-{command}-{uuid.uuid4().hex[:4]}.log").write_text(
            result.stdout + "\n--- stderr ---\n" + result.stderr)
        if check and result.returncode != 0:
            raise RuntimeError(f"driver {command} failed: {result.stderr[-2000:]}")
        return result

    def bind_project(self, member, role, condition=None):
        args = ["projects", "add-iam-policy-binding", self.project, f"--member={member}", f"--role={role}"]
        args.append(f"--condition={condition}" if condition else "--condition=None")
        self.gcloud(*args)
        self.bindings.append((member, role, condition))

    # --- setup ------------------------------------------------------------------

    def push_images(self):
        self.gcloud("artifacts", "repositories", "create", self.prefix, "--repository-format=docker",
                    f"--location={self.region}", f"--labels={self.label_key}={self.label_value}")
        self.created.append(("repository", self.prefix))
        subprocess.run(["docker", "login", "-u", "oauth2accesstoken", "--password-stdin",
                        f"https://{self.registry_host}"], input=self.token().encode(), check=True,
                       capture_output=True)
        self.images = {}
        for tag, source in [("a", "nginxinc/nginx-unprivileged:1.27-alpine"),
                            ("b", "nginxinc/nginx-unprivileged:1.26-alpine")]:
            for step in (["pull", "--platform", "linux/amd64", source],
                         ["tag", source, f"{self.repo}/api:{tag}"], ["push", f"{self.repo}/api:{tag}"]):
                subprocess.run(["docker", *step], check=True, capture_output=True)
            digest = subprocess.run(["crane", "digest", f"{self.repo}/api:{tag}"], check=True,
                                    capture_output=True, text=True).stdout.strip()
            self.images[tag] = f"{self.repo}/api@{digest}"
        context = self.work / "verifier-image"
        context.mkdir()
        subprocess.run(["strip", "-o", str(context / "verifier"), self.args.verifier], check=True)
        (context / "Dockerfile").write_text(
            "FROM public.ecr.aws/docker/library/alpine:3.20\nCOPY verifier /verifier\nENTRYPOINT [\"/verifier\"]\n")
        subprocess.run(["docker", "build", "--platform", "linux/amd64", "-t", f"{self.repo}/verifier:e2e",
                        str(context)], check=True, capture_output=True)
        subprocess.run(["docker", "push", f"{self.repo}/verifier:e2e"], check=True, capture_output=True)
        digest = subprocess.run(["crane", "digest", f"{self.repo}/verifier:e2e"], check=True,
                                capture_output=True, text=True).stdout.strip()
        self.verifier_image = f"{self.repo}/verifier@{digest}"

    def accounts(self):
        for name in ("verifier", "execution-sa", "mgmt"):
            self.gcloud("iam", "service-accounts", "create", f"{self.prefix}-{name}",
                        f"--display-name={self.prefix} {name}")
            self.created.append(("account", self.sa[name]))
        time.sleep(10)
        self.exec_id = self.gcloud_json("iam", "service-accounts", "describe", self.sa["execution-sa"])["uniqueId"]
        verifier = f"serviceAccount:{self.sa['verifier']}"
        mgmt = f"serviceAccount:{self.sa['mgmt']}"
        # The verifier's Cloud Run grants as setup renders them: one custom role, bound without a
        # condition (Cloud Run doesn't evaluate resource.name conditions).
        role_id = self.prefix.replace("-", "_") + "_cloud_run"
        self.gcloud("iam", "roles", "create", role_id, f"--title={self.prefix} cloud run",
                    "--permissions=run.services.create,run.services.update,run.services.delete,"
                    "run.services.get,run.services.getIamPolicy,run.services.setIamPolicy")
        self.created.append(("role", role_id))
        self.bind_project(verifier, f"projects/{self.project}/roles/{role_id}")
        self.bind_project(verifier, "roles/parametermanager.parameterViewer")
        self.bind_project(verifier, "roles/artifactregistry.reader")
        ledger = f"projects/{self.project}/locations/global/parameters/signed-{self.prefix}-state"
        self.bind_project(verifier, "roles/parametermanager.parameterVersionAdder",
                          f"expression=resource.name.startsWith(\"{ledger}\"),title=ledger")
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.sa["execution-sa"],
                    f"--member={verifier}", "--role=roles/iam.serviceAccountUser")
        # Management may only read Cloud Run; it calls the verifier with its own ID token.
        self.bind_project(mgmt, "roles/run.viewer")
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.sa["mgmt"],
                    f"--member={mgmt}", "--role=roles/iam.serviceAccountTokenCreator")
        account = subprocess.run(["curl", "-s", "-H", f"Authorization: Bearer {self.token()}",
                                  "https://oauth2.googleapis.com/tokeninfo?access_token=" + self.token()],
                                 capture_output=True, text=True).stdout
        self.caller = json.loads(account)["email"]
        self.gcloud("iam", "service-accounts", "add-iam-policy-binding", self.sa["mgmt"],
                    f"--member=user:{self.caller}", "--role=roles/iam.serviceAccountTokenCreator")

    def parameters(self):
        fixtures = json.loads(self.driver("fixtures").stdout)
        for name, value in [(f"signed-{self.prefix}-fingerprint", fixtures["settings"]),
                            (f"signed-{self.prefix}-state", fixtures["ledger"])]:
            path = self.work / f"{name}.json"
            path.write_text(json.dumps(value))
            self.gcloud("parametermanager", "parameters", "create", name, "--location=global",
                        f"--labels={self.label_key}={self.label_value}")
            self.created.append(("parameter", name))
            self.gcloud("parametermanager", "parameters", "versions", "create", "v1", f"--parameter={name}",
                        "--location=global", f"--payload-data-from-file={path}")

    def verifier(self):
        self.gcloud("run", "deploy", f"{self.prefix}-verifier", f"--image={self.verifier_image}",
                    f"--region={self.region}", f"--service-account={self.sa['verifier']}",
                    "--no-allow-unauthenticated", "--max-instances=1",
                    "--set-env-vars=VERIFIER_CLOUD=gcp,RUST_LOG=info",
                    f"--labels={self.label_key}={self.label_value}")
        self.created.append(("service", f"{self.prefix}-verifier"))
        self.gcloud("run", "services", "add-iam-policy-binding", f"{self.prefix}-verifier",
                    f"--region={self.region}", f"--member=serviceAccount:{self.sa['mgmt']}",
                    "--role=roles/run.invoker")
        log("waiting 90s for IAM to propagate")
        time.sleep(90)

    # --- cases ------------------------------------------------------------------

    def service(self):
        return self.gcloud_json("run", "services", "describe", f"{self.prefix}-api",
                                f"--region={self.region}", check=False)

    def image(self):
        service = self.service()
        containers = service.get("spec", {}).get("template", {}).get("spec", {}).get("containers", [{}])
        return containers[0].get("image", "")

    def outcome(self, result):
        lines = result.stdout.strip().splitlines()
        return json.loads(lines[-1]) if lines else {}

    def cases(self):
        self.created.append(("service", f"{self.prefix}-api"))
        result = self.driver("update", {"FROM_IMAGE": "", "TO_IMAGE": self.images["a"], "PROTECTED": "1",
                                        "SEQUENCE": "1"}, check=False)
        outcome = self.outcome(result)
        service = self.service()
        ingress = service.get("metadata", {}).get("annotations", {}).get("run.googleapis.com/ingress")
        policy = self.gcloud_json("run", "services", "get-iam-policy", f"{self.prefix}-api",
                                  f"--region={self.region}", check=False)
        invoker = any(binding.get("role") == "roles/run.invoker"
                      and f"serviceAccount:{self.sa['execution-sa']}" in binding.get("members", [])
                      for binding in policy.get("bindings", []))
        self.record("0 the first deployment creates the worker's service through the verifier",
                    result.returncode == 0 and outcome.get("outcome") == "synced"
                    and self.image() == self.images["a"] and ingress == "internal" and invoker,
                    f"driver={outcome} image={self.image()[-19:]} ingress={ingress} invoker={invoker}")

        result = self.driver("update", {"FROM_IMAGE": self.images["a"], "TO_IMAGE": self.images["b"],
                                        "PROTECTED": "1", "SEQUENCE": "2"}, check=False)
        outcome = self.outcome(result)
        image = self.image()
        direct = self.gcloud("run", "services", "update", f"{self.prefix}-api", f"--region={self.region}",
                             f"--image={self.images['a']}", check=False,
                             token_file=self.management_token_file())
        self.record("1 an update goes through the verifier; management can't update the service itself",
                    result.returncode == 0 and outcome.get("outcome") == "synced" and image == self.images["b"]
                    and direct.returncode != 0 and "'run.services.update' denied" in direct.stderr,
                    f"driver={outcome} image={image[-19:]} direct={direct.stderr.strip()[-300:]}")

        foreign_after = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=20)) \
            .replace(microsecond=0).isoformat()
        result = self.driver("update", {"FROM_IMAGE": self.images["b"], "TO_IMAGE": self.images["a"],
                                        "PROTECTED": "1", "SEQUENCE": "3", "NOT_AFTER": foreign_after},
                             check=False)
        outcome = self.outcome(result)
        self.record("2 a release under a signer list setup didn't pin is refused with the reason",
                    result.returncode != 0 and outcome.get("code") == "RELEASE_REFUSED_BY_VERIFIER"
                    and "does not match the pinned fingerprint" in json.dumps(outcome)
                    and self.image() == self.images["b"],
                    f"driver={json.dumps(outcome)[:400]} image={self.image()[-19:]}")

    # --- cleanup ----------------------------------------------------------------

    def cleanup(self):
        logs = self.gcloud("logging", "read",
                           f'resource.type="cloud_run_revision" AND resource.labels.service_name="{self.prefix}-verifier"',
                           "--limit=200", "--freshness=2h", check=False)
        (self.evidence / "verifier-log.json").write_text(logs.stdout)
        for kind, name in reversed(self.created):
            if kind == "service":
                self.gcloud("run", "services", "delete", name, f"--region={self.region}", check=False)
            elif kind == "parameter":
                for version in self.gcloud_json("parametermanager", "parameters", "versions", "list",
                                                f"--parameter={name}", "--location=global", check=False) or []:
                    self.gcloud("parametermanager", "parameters", "versions", "delete",
                                version["name"].rsplit("/", 1)[-1], f"--parameter={name}",
                                "--location=global", check=False)
                self.gcloud("parametermanager", "parameters", "delete", name, "--location=global", check=False)
            elif kind == "repository":
                self.gcloud("artifacts", "repositories", "delete", name, f"--location={self.region}", check=False)
        for member, role, condition in self.bindings:
            self.gcloud("projects", "remove-iam-policy-binding", self.project, f"--member={member}",
                        f"--role={role}", f"--condition={condition}" if condition else "--condition=None",
                        check=False)
        for kind, name in reversed(self.created):
            if kind == "account":
                self.gcloud("iam", "service-accounts", "delete", name, check=False)
            elif kind == "role":
                self.gcloud("iam", "roles", "delete", name, check=False)

    def audit(self):
        time.sleep(20)  # account deletion settles before it disappears from the list
        left = []
        left += [s["metadata"]["name"] for s in self.gcloud_json("run", "services", "list",
                 f"--region={self.region}") if s["metadata"]["name"].startswith(self.prefix)]
        left += [a["email"] for a in self.gcloud_json("iam", "service-accounts", "list")
                 if a["email"].startswith(self.prefix)]
        left += [p["name"] for p in self.gcloud_json("parametermanager", "parameters", "list",
                 "--location=global") if f"signed-{self.prefix}-" in p["name"]]
        left += [r["name"] for r in self.gcloud_json("artifacts", "repositories", "list",
                 f"--location={self.region}") if r["name"].endswith(f"/{self.prefix}")]
        left += [r["name"] for r in self.gcloud_json("iam", "roles", "list")
                 if self.prefix.replace("-", "_") in r["name"] and not r.get("deleted")]
        policy = self.gcloud_json("projects", "get-iam-policy", self.project)
        left += [f"{b['role']} {m}" for b in policy.get("bindings", []) for m in b.get("members", [])
                 if self.prefix in m]
        self.record("cleanup: nothing named after the run is left", not left, f"left={left}")

    def run(self):
        log(f"run {self.prefix}")
        try:
            self.project_number = self.gcloud_json("projects", "describe", self.project)["projectNumber"]
            self.push_images()
            self.accounts()
            self.parameters()
            self.verifier()
            self.cases()
        finally:
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
    parser.add_argument("--project", required=True)
    parser.add_argument("--region", default="us-central1")
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--prefix", default="e2e-cpg")
    parser.add_argument("--label", default="purpose=control-plane-e2e")
    parser.add_argument("--evidence", required=True)
    sys.exit(Run(parser.parse_args()).run())


if __name__ == "__main__":
    main()
