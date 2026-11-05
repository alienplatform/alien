#!/usr/bin/env python3
"""rollMachines on real GCP, through the verifier's own GCP code.

Runs the `verifier-local` example (the deployed verifier's stores, backend
and checks, with the identity and pins given on its command line) against a
test project. Creates, named `$TEST_RESOURCE_PREFIX-<random>` and labeled
`<prefix>=true` where GCP allows labels:
  - the signing settings and ledger as Parameter Manager parameters
    `signed-<p>-fingerprint` and `signed-<p>-state`, as setup writes them;
  - the machines' service account `<p>-m`;
and lets the verifier create the instance templates and the zonal MIG.

The test build pins `debian-cloud` as the image publisher and an oldest
image date, so a Debian image boots and an Ubuntu one stands in for "not
the publisher's". Then, each refusal checked to write nothing:
  1. a signed release rolls: template `<p>-compute-general-1` from the signed
     group (machine type, setup's subnetwork, network tags and service
     account, the capacity group in metadata, no user-data), MIG at the
     signed minimum, and the MIG creates a machine from it;
  2. an image from another project is refused;
  3. a subnetwork or network tag that isn't setup's is refused (off: the same
     tag written straight into an instance template lands);
  4. a zone outside the verifier's region is refused;
  5. a publisher image older than the build's minimum is refused;
  6. a second roll adds template 2 and points the MIG at it;
  7. each roll writes the ledger back as a new parameter version;
  8. `scale` resizes the MIG the verifier names, clamped to the signed
     bounds; a caller-named target or a foreign zone is refused (off: a
     resize past the maximum written straight to Compute Engine lands);
  9. `removeMachines` deletes only members and never below the signed
     minimum (off: deleting the last machine directly lands);
  10. a disabled newest ledger version refuses; enabled again, it works.

Usage (an access token for the project in --token-file):
    scripts/gate-e2e/gcp_machines.py --verifier-local <example> --fixtures <gate-fixtures> \\
        --project <project> --region us-central1 --token-file <file> --evidence <dir>
"""

import argparse
import base64
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid

PUBLISHER = "debian-cloud"
MINIMUM_CREATED = "2025-01-01T00:00:00Z"


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class Run:
    def __init__(self, args):
        self.args = args
        prefix = os.environ.get("TEST_RESOURCE_PREFIX") or args.prefix
        if not prefix:
            raise SystemExit("set TEST_RESOURCE_PREFIX")
        self.label = prefix
        self.prefix = f"{prefix}-{uuid.uuid4().hex[:6]}"
        self.project, self.region, self.zone = args.project, args.region, f"{args.region}-a"
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="gcp-machines-"))
        self.account = f"{self.prefix}-m"
        self.email = f"{self.account}@{self.project}.iam.gserviceaccount.com"
        self.group = f"{self.prefix}-compute-general"
        self.subnetwork = f"projects/{self.project}/regions/{self.region}/subnetworks/default"
        self.tag = f"{self.prefix}-machines"
        self.results, self.calls = [], []

    def call(self, method, url, body=None, ok_missing=False):
        token = pathlib.Path(self.args.token_file).read_text().strip()
        request = urllib.request.Request(url, method=method, data=json.dumps(body).encode() if body is not None else None,
                                         headers={"Authorization": f"Bearer {token}", "content-type": "application/json"})
        try:
            with urllib.request.urlopen(request) as response:
                text = response.read()
                return json.loads(text) if text else {}
        except urllib.error.HTTPError as error:
            if ok_missing and error.code == 404:
                return None
            raise RuntimeError(f"{method} {url}: HTTP {error.code}: {error.read().decode()[:500]}")

    def compute(self, method, path, body=None, ok_missing=False):
        return self.call(method, f"https://compute.googleapis.com/compute/v1/{path}", body, ok_missing)

    def wait(self, operation):
        while operation.get("status") != "DONE":
            time.sleep(2)
            operation = self.call("GET", operation["selfLink"])
        if "error" in operation:
            raise RuntimeError(json.dumps(operation["error"]))

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    # --- setup ------------------------------------------------------------

    def images(self):
        family = self.compute("GET", "projects/debian-cloud/global/images/family/debian-12")
        self.image = f"projects/debian-cloud/global/images/{family['name']}"
        other = self.compute("GET", "projects/ubuntu-os-cloud/global/images/family/ubuntu-2204-lts")
        self.other_image = f"projects/ubuntu-os-cloud/global/images/{other['name']}"
        listed = self.compute("GET", "projects/debian-cloud/global/images?filter=" + urllib.request.quote('name eq "debian-12-bookworm-v2024.*"') + "&showDeprecated=true")
        old = sorted((i for i in listed.get("items", []) if i["creationTimestamp"] < MINIMUM_CREATED), key=lambda i: i["creationTimestamp"])
        self.old_image = f"projects/debian-cloud/global/images/{old[0]['name']}" if old else None

    def fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), self.prefix, f"gcp:{self.project}:{self.prefix}",
                        "unused@sha256:" + "0" * 64, "unused@sha256:" + "1" * 64], check=True, capture_output=True)
        self.statements = {p.stem: json.loads(p.read_text()) for p in out.glob("*.json")}

    def parameters(self):
        base = f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/parameters"
        for name, value in [(f"signed-{self.prefix}-fingerprint", self.statements["settings"]), (f"signed-{self.prefix}-state", self.statements["ledger"])]:
            self.call("POST", f"{base}?parameterId={name}", {"labels": {self.label: "true"}})
            self.call("POST", f"{base}/{name}/versions?parameterVersionId=v1",
                      {"payload": {"data": base64.b64encode(json.dumps(value).encode()).decode()}})

    def service_account(self):
        self.call("POST", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts",
                  {"accountId": self.account, "serviceAccount": {"displayName": "machines e2e"}})

    # --- calls --------------------------------------------------------------

    def invoke(self, kind="rollMachines", **overrides):
        base = {"cluster": "compute", "group": "general",
                "release": self.statements["release-71-machines-gcp"],
                "signerLists": [self.statements["signer-list"]], "zone": self.zone}
        if kind == "rollMachines":
            base["machineImage"] = self.image
        request = {kind: {**base, **overrides}}
        network = json.dumps({"subnetIds": [self.subnetwork], "securityGroupIds": [self.tag], "identity": self.email})
        result = subprocess.run([self.args.verifier_local, "gcp", self.project, self.region, f"{self.prefix}-verifier",
                                 PUBLISHER, MINIMUM_CREATED, network], input=json.dumps(request), capture_output=True, text=True,
                                env={**os.environ, "VERIFIER_GCP_TOKEN_FILE": self.args.token_file})
        body = json.loads(result.stdout or "{}")
        self.calls.append({"request": request, "ok": result.returncode == 0, "response": body, "stderr": result.stderr[-2000:]})
        return result.returncode == 0, body if result.returncode == 0 else body.get("errorMessage", result.stderr[-500:])

    def members(self):
        listed = self.compute("POST", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}/listManagedInstances", {})
        return sorted(i["instance"].rsplit("/", 1)[-1] for i in listed.get("managedInstances", []))

    def settle(self, size):
        """Wait until the MIG is stable with `size` members."""
        for _ in range(60):
            mig = self.mig()
            if mig["status"].get("isStable") and len(self.members()) == size:
                return True
            time.sleep(5)
        return False

    def ledger_versions(self):
        base = f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/parameters/signed-{self.prefix}-state"
        versions = self.call("GET", f"{base}/versions").get("parameterVersions", [])
        return sorted(versions, key=lambda v: v["createTime"])

    def templates(self):
        listed = self.compute("GET", f"projects/{self.project}/global/instanceTemplates?filter=" + urllib.request.quote(f'name eq "{self.group}-[0-9]+"'))
        return sorted(t["name"] for t in listed.get("items", []))

    def mig(self):
        return self.compute("GET", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}", ok_missing=True)

    def expect_refused(self, name, reason, **overrides):
        before = self.templates()
        ok, body = self.invoke(**overrides)
        unchanged = self.templates() == before
        self.record(name, not ok and reason in str(body) and unchanged, f"ok={ok} unchanged={unchanged} reason={str(body)[:400]}")

    def run(self):
        self.images()
        self.fixtures()
        self.parameters()
        self.service_account()
        time.sleep(10)
        log(f"ready: prefix {self.prefix}, image {self.image}")

        self.expect_refused("2 an image from another project (on)", f"not the publisher {PUBLISHER}", machineImage=self.other_image)
        self.expect_refused("3 a subnetwork that isn't setup's (on)", "is not one setup created",
                            subnetIds=[f"projects/{self.project}/regions/{self.region}/subnetworks/other"])
        self.expect_refused("3 a network tag that isn't setup's (on)", "is not one setup created", securityGroupIds=["allow-all"])
        self.expect_refused("4 a zone outside the verifier's region (on)", "is not a zone of the verifier's region", zone="europe-west1-b")
        if self.old_image:
            self.expect_refused("5 a publisher image older than the minimum (on)", "the oldest image this build boots", machineImage=self.old_image)
        else:
            self.record("5 a publisher image older than the minimum (on)", False, "no older debian-12 image is listed")
        self.record("2-5 nothing was created", self.templates() == [] and self.mig() is None, f"templates={self.templates()}")

        ok, body = self.invoke()
        self.record("1 a signed release rolls", ok and body == {"template": f"{self.group}-1", "version": 1, "group": self.group}, str(body)[:300])
        template = self.compute("GET", f"projects/{self.project}/global/instanceTemplates/{self.group}-1")["properties"]
        metadata = {item["key"]: item.get("value") for item in template.get("metadata", {}).get("items", [])}
        self.record("1 the template is the signed group's", template["machineType"] == "e2-small"
                    and template["disks"][0]["initializeParams"]["sourceImage"].endswith(self.image)
                    and template["networkInterfaces"][0]["subnetwork"].endswith(self.subnetwork)
                    and template["serviceAccounts"][0]["email"] == self.email and template["tags"]["items"] == [self.tag]
                    and metadata.get("block-project-ssh-keys") == "true" and "user-data" not in metadata and "startup-script" not in metadata
                    and len(metadata) == 2, json.dumps(template)[:700])
        mig = self.mig()
        self.record("1 the MIG starts at the signed minimum", mig is not None and mig["targetSize"] == 1
                    and mig["instanceTemplate"].endswith(f"instanceTemplates/{self.group}-1") and mig["updatePolicy"]["type"] == "OPPORTUNISTIC",
                    json.dumps({k: (mig or {}).get(k) for k in ("targetSize", "instanceTemplate", "updatePolicy")}))
        instance = None
        for _ in range(40):
            listed = self.compute("POST", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}/listManagedInstances", {})
            created = [i for i in listed.get("managedInstances", []) if i.get("instanceStatus") in ("RUNNING", "STAGING", "PROVISIONING")]
            if created:
                instance = self.call("GET", created[0]["instance"])
                break
            time.sleep(6)
        self.record("1 the MIG created a machine from the template", instance is not None and instance["machineType"].endswith("/e2-small")
                    and instance["networkInterfaces"][0]["subnetwork"].endswith(self.subnetwork), json.dumps({k: (instance or {}).get(k) for k in ("name", "machineType", "status")}))

        # 3 off: the same foreign tag written straight into a template lands.
        operation = self.compute("POST", f"projects/{self.project}/global/instanceTemplates",
                                 {**self.compute("GET", f"projects/{self.project}/global/instanceTemplates/{self.group}-1"),
                                  "name": f"{self.prefix}-direct", "properties": {**template, "tags": {"items": ["allow-all"]}}})
        self.wait(operation)
        direct = self.compute("GET", f"projects/{self.project}/global/instanceTemplates/{self.prefix}-direct")
        self.record("3 a network tag that isn't setup's (off: written straight to Compute Engine)", direct["properties"]["tags"]["items"] == ["allow-all"], "landed")

        ok, body = self.invoke()
        mig = self.mig()
        self.record("6 a second roll adds template 2 and points the MIG at it", ok and body.get("version") == 2
                    and mig["instanceTemplate"].endswith(f"instanceTemplates/{self.group}-2"), str(body)[:300])

        # 7 the ledger: the roll wrote a new version holding the release.
        versions = self.ledger_versions()
        newest = json.loads(base64.b64decode(self.call("GET", f"https://parametermanager.googleapis.com/v1/{versions[-1]['name']}")["payload"]["data"]))
        self.record("7 the roll wrote the ledger back as a new parameter version", len(versions) >= 2
                    and newest.get("release", {}).get("sequence") == 71, json.dumps({"versions": len(versions), "release": newest.get("release")}))

        # 8 scale: clamped to the signed bounds, in the group the verifier names.
        self.settle(1)
        ok, body = self.invoke("scale", desired=5)
        self.record("8 scale to 5 resizes to the signed maximum 2 (on)", ok and body == {"target": self.group, "size": 2}
                    and self.mig()["targetSize"] == 2, str(body)[:300])
        ok, body = self.invoke("scale", desired=0)
        self.record("8 scale to 0 resizes to the signed minimum 1 (on)", ok and body.get("size") == 1 and self.mig()["targetSize"] == 1, str(body)[:300])
        before = self.mig()["targetSize"]
        ok, body = self.invoke("scale", desired=2, target=f"{self.prefix}-other")
        self.record("8 a caller-named scale target is refused (on)", not ok and "target" in str(body) and self.mig()["targetSize"] == before, str(body)[:300])
        ok, body = self.invoke("scale", desired=2, zone="europe-west1-b")
        self.record("8 a scale zone outside the verifier's region is refused (on)", not ok and "is not a zone of the verifier's region" in str(body)
                    and self.mig()["targetSize"] == before, str(body)[:300])
        self.wait(self.compute("POST", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}/resize?size=3"))
        self.record("8 a resize past the signed maximum (off: written straight to Compute Engine)", self.mig()["targetSize"] == 3, "landed: targetSize 3")

        # 9 removeMachines: only members, never below the signed minimum.
        ok, body = self.invoke("scale", desired=2)
        settled = self.settle(2)
        members = self.members()
        self.record("9 two machines to remove from", ok and settled and len(members) == 2, json.dumps(members))
        ok, body = self.invoke("removeMachines", machines=[f"{self.prefix}-not-a-member"])
        self.record("9 removing a machine that isn't a member is refused (on)", not ok and "not a member" in str(body)
                    and self.members() == members, str(body)[:300])
        ok, body = self.invoke("removeMachines", machines=members)
        self.record("9 removing both machines (below the signed minimum 1) is refused (on)", not ok and "minimum" in str(body)
                    and self.members() == members, str(body)[:300])
        ok, body = self.invoke("removeMachines", machines=members[:1])
        gone = self.settle(1) and self.members() == members[1:]
        self.record("9 removing one member lands", ok and body == {"group": self.group, "removed": members[:1], "size": 1} and gone,
                    json.dumps({"body": body, "members": self.members()}))
        ok, body = self.invoke("removeMachines", machines=members[1:])
        self.record("9 removing the last machine is refused (on)", not ok and "minimum" in str(body) and self.members() == members[1:], str(body)[:300])
        self.wait(self.compute("POST", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}/deleteInstances",
                               {"instances": [f"zones/{self.zone}/instances/{members[1]}"], "skipInstancesOnValidationError": False}))
        self.record("9 removing the last machine (off: written straight to Compute Engine)", self.mig()["targetSize"] == 0, "landed: targetSize 0")

        # 10 a disabled newest ledger version is an error, never a skip.
        newest = self.ledger_versions()[-1]["name"]
        self.call("PATCH", f"https://parametermanager.googleapis.com/v1/{newest}?updateMask=disabled", {"disabled": True})
        ok, body = self.invoke("scale", desired=1)
        self.record("10 a disabled newest ledger version refuses (on)", not ok and "is disabled" in str(body) and self.mig()["targetSize"] == 0, str(body)[:300])
        self.call("PATCH", f"https://parametermanager.googleapis.com/v1/{newest}?updateMask=disabled", {"disabled": False})
        ok, body = self.invoke("scale", desired=1)
        self.record("10 enabled again, scale lands", ok and body.get("size") == 1, str(body)[:300])

    # --- teardown -------------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        try:
            if self.mig() is not None:
                self.wait(self.compute("DELETE", f"projects/{self.project}/zones/{self.zone}/instanceGroupManagers/{self.group}"))
        except Exception as error:
            log(f"MIG delete: {error}")
        for name in self.templates() + [f"{self.prefix}-direct"]:
            try:
                operation = self.compute("DELETE", f"projects/{self.project}/global/instanceTemplates/{name}", ok_missing=True)
                if operation:
                    self.wait(operation)
            except Exception as error:
                log(f"template {name}: {error}")
        base = f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/parameters"
        for name in (f"signed-{self.prefix}-fingerprint", f"signed-{self.prefix}-state"):
            for version in (self.call("GET", f"{base}/{name}/versions", ok_missing=True) or {}).get("parameterVersions", []):
                self.call("DELETE", f"https://parametermanager.googleapis.com/v1/{version['name']}", ok_missing=True)
            self.call("DELETE", f"{base}/{name}", ok_missing=True)
        self.call("DELETE", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{self.email}", ok_missing=True)

    def audit(self):
        left = []
        if self.mig() is not None:
            left.append(f"mig {self.group}")
        left += [f"template {t}" for t in self.templates()]
        if self.compute("GET", f"projects/{self.project}/global/instanceTemplates/{self.prefix}-direct", ok_missing=True):
            left.append("template direct")
        # IAM reads lag a delete by a few seconds.
        for _ in range(12):
            if not self.call("GET", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{self.email}", ok_missing=True):
                break
            time.sleep(5)
        else:
            left.append(f"service account {self.email}")
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--verifier-local", required=True)
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--region", default="us-central1")
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--prefix", help="when TEST_RESOURCE_PREFIX is unset")
    args = parser.parse_args()
    run = Run(args)
    try:
        run.run()
    except Exception as error:
        run.record("the run itself", False, f"{type(error).__name__}: {error}")
    finally:
        (run.evidence / "results.json").write_text(json.dumps(run.results, indent=2))
        (run.evidence / "calls.json").write_text(json.dumps(run.calls, indent=2))
        run.cleanup()
        left = run.audit()
        (run.evidence / "audit.json").write_text(json.dumps(left))
        log(f"left after cleanup: {left or 'nothing'}")
    failed = [r for r in run.results if not r["pass"]]
    log(f"{len(run.results) - len(failed)}/{len(run.results)} passed")
    sys.exit(1 if failed or not run.results else 0)


if __name__ == "__main__":
    main()
