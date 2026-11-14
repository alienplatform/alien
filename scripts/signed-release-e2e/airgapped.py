#!/usr/bin/env python3
"""Air-gapped attack suite for protected mode (signed releases).

The same environment as `run.py` (a real manager, a kind cluster, the proxy
playing a compromised manager), but the cluster is an air-gapped site: it
never reaches the manager. `alien-deploy sync` runs twice per trip, as two
machines with their own settings: online, through the proxy, it downloads an
update the manager signs; at the site it installs it into the site's own
registry and cluster. The manager signs whatever bundle the online side asks
it to, so every attack arrives with a valid bundle signature.

    python3 scripts/signed-release-e2e/airgapped.py --mode protected
    python3 scripts/signed-release-e2e/airgapped.py --mode unprotected

`protected` pins the release key in the site's values and expects each attack
to change nothing; `unprotected` expects each to take effect. Either way a
normal signed release still installs with no admin action, and a rollback is
a re-release.

The app is released for two platforms, so its signed image is a multi-platform
index, while the site declares one architecture: the signed index must still run.

Needs what `run.py` needs, plus a debug build of `alien-deploy` (`cargo build
-p alien-deploy-cli`). Run `run.py --down` first if its environment is up;
`--down` here removes both.
"""

import argparse
import base64
import copy
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.dont_write_bytecode = True  # importing run.py leaves no __pycache__ in the repo
sys.path.insert(0, str(Path(__file__).resolve().parent))
import run as connected  # noqa: E402
from run import (  # noqa: E402
    CONTEXT, CUSTOMER_DATABASE_URL, MANAGER_PORT, PROXY_PORT, REPO,
    kubectl, log, run, wait_until,
)

# The site's registry is run.py's registry container, under `vendor/`.
SITE_REGISTRY_HOST = connected.REGISTRY_HOST
SITE = "site"
SITE_SYNC_TIMEOUT = 300
# A builder that declares arm64 as well. The app's Dockerfile runs nothing at
# build time, so it builds for arm64 without emulation.
BUILDER = "sign-e2e-multi"


class AirgapSuite(connected.Suite):
    def __init__(self, args):
        super().__init__(args)
        self.alien_deploy = Path(args.alien_deploy or self.binaries / "alien-deploy")
        self.trips = self.work / "airgap"
        self.trips.mkdir()
        self.trip_log = []
        connected.NAMESPACE = SITE
        # Two platforms, so each signed image is an index.
        self.targets = "linux-x64,linux-arm64"

    def up(self):
        super().up()
        if run(["docker", "buildx", "inspect", BUILDER], check=False).returncode != 0:
            run(["docker", "buildx", "create", "--name", BUILDER, "--driver", "docker-container",
                 "--platform", "linux/amd64,linux/arm64"])

    def down(self):
        super().down()
        run(["docker", "buildx", "rm", "--force", BUILDER], check=False)

    def cli_env(self):
        return {**super().cli_env(), "BUILDX_BUILDER": BUILDER}

    # ── The two machines ─────────────────────────────────────────────────

    def deploy_cli(self, machine, *args, timeout=None, check=True):
        """`alien-deploy` as the online machine or the site's, each with its own settings."""
        env = {**os.environ, "XDG_CONFIG_HOME": str(self.work / f"{machine}-config"), "NO_COLOR": "1"}
        try:
            result = subprocess.run([str(self.alien_deploy), *map(str, args)], cwd=self.trips, env=env,
                                    text=True, capture_output=True, timeout=timeout)
            output, code = result.stdout + result.stderr, result.returncode
        except subprocess.TimeoutExpired as expired:
            output = (expired.stdout or b"").decode() + (expired.stderr or b"").decode()
            code = "timeout"
        self.trip_log.append({"machine": machine, "args": [str(a) for a in args], "exit": code,
                              "output": output})
        if check and code != 0:
            raise RuntimeError(f"alien-deploy {' '.join(map(str, args))} ({machine}) failed ({code}):\n{output}")
        return code, output

    def folder(self):
        return next(self.trips.glob("*-sync"))

    def online(self):
        if not any(self.trips.glob("*-sync")):
            return self.deploy_cli("online", "sync", "--token", self.onboard["token"],
                                   "--manager", f"http://localhost:{PROXY_PORT}", "-f", self.values_file,
                                   "--site-architecture", "amd64", "--site-tool-platform", "linux-x86_64")
        return self.deploy_cli("online", "sync", "--folder", self.folder())

    def site(self, *extra, check=True):
        # The site's settings, on every run: remembered ones are the same.
        args = ["sync", "--folder", self.folder()]
        args += ["--registry", f"{SITE_REGISTRY_HOST}/vendor", "--insecure-registry", "--namespace", SITE,
                 "-f", self.values_file, "--kube-context", CONTEXT,
                 "--trusted-key", self.onboard["bundleSigningKey"]]
        return self.deploy_cli("site", *args, *extra, timeout=SITE_SYNC_TIMEOUT, check=check)

    def trip(self, check=True):
        """Online download, site install, then online again to carry the report back."""
        self.online()
        result = self.site(check=check)
        self.online()
        return result

    def first_install(self):
        # A site left by an earlier run trusts another manager's bundle key.
        kubectl("delete", "namespace", SITE, "--ignore-not-found", "--wait=true", "--timeout=180s")
        self.onboard = json.loads(self.alien_cli(
            "onboard", SITE, "--platforms", "kubernetes", "--airgapped", "--json").stdout)
        values = {"inputValues": {"databaseUrl": CUSTOMER_DATABASE_URL}}
        if self.protected:
            values["releaseSigning"] = {"publicKeys": [self.public_key], "project": "default"}
        self.values_file = self.work / "site-values.yaml"
        self.values_file.write_text(json.dumps(values))
        self.save("site-values.json", values)
        self.online()
        extra = []
        if self.protected:
            # The site admin reviews the chart, then accepts its digest.
            code, output = self.site("--dry-run", check=False)
            refused = self.site(check=False)
            self.save("first-install-without-accept.txt", refused[1])
            assert refused[0] != 0 and "--accept-chart" in refused[1], refused
            self.chart_digest = re.search(r"--accept-chart (sha256:[0-9a-f]{64})", refused[1]).group(1)
            assert self.chart_digest in output, output
            extra = ["--accept-chart", self.chart_digest]
        self.site(*extra)
        self.online()
        # The chart is installed under the stack's name; find its Operator.
        deployments = json.loads(kubectl("-n", SITE, "get", "deployments", "-o", "json").stdout)["items"]
        connected.RELEASE_NAME = next(
            d["metadata"]["name"] for d in deployments
            if any("alien-operator" in c["image"] for c in d["spec"]["template"]["spec"]["containers"]))

    def check_multi_platform_image(self, since):
        """The app's signed image is an index; the site runs that same digest from its registry."""
        name = "multi-platform-image"
        signed = self.releases[-1]["image"].split("@")[1]
        media_type = json.loads(run(["curl", "-sf", "-H", "Accept: application/vnd.oci.image.index.v1+json,"
                                     "application/vnd.oci.image.manifest.v1+json",
                                     f"http://localhost:{MANAGER_PORT}/v2/artifacts/default/manifests/{signed}",
                                     "-u", f"admin:{self.admin_token}"]).stdout).get("mediaType")

        def running():
            workload = self.app_workload()
            ready = workload and workload.get("status", {}).get("readyReplicas")
            return ready and workload["spec"]["template"]["spec"]["containers"][0]["image"]
        try:
            image = wait_until("the app to run", running, timeout=240)
        except TimeoutError:
            image = None
        passed = (media_type == "application/vnd.oci.image.index.v1+json" and bool(image)
                  and image.startswith(SITE_REGISTRY_HOST)
                  # Unprotected, the site still narrows the index to its platform's manifest.
                  and (image.endswith(signed) or not self.protected))
        result = {"attack": name, "summary": "a signed multi-platform index runs at a site with a known architecture",
                  "signed": signed, "signed_media_type": media_type, "image": image,
                  "took_effect": bool(image), "passed": passed}
        self.save(f"attacks/{name}/operator.log", self.operator_logs(since))
        self.save(f"attacks/{name}/result.json", result)
        log(f"{name}: {'PASS' if passed else 'FAIL'} (signed {media_type} {signed}, running {image})")
        self.results.append(result)

    def operator_env(self):
        workload = self.operator_workload()
        return {e["name"]: e.get("value") for c in workload["spec"]["template"]["spec"]["containers"]
                for e in c.get("env", [])}

    def manager_target(self):
        return json.loads(run(["curl", "-sf", "-H", f"Authorization: Bearer {self.onboard['token']}",
                               f"http://localhost:{MANAGER_PORT}/v1/deployments/"
                               f"{self.onboard['deploymentId']}/target"]).stdout)

    def released(self, message):
        """`alien release` of `message`, signed, and the target the manager then serves the site."""
        self.release(message, signed=True)
        release_id = self.releases[-1]["id"]
        return wait_until(f"the manager to target {message}", lambda: (
            lambda t: t if t.get("releaseInfo", {}).get("releaseId") == release_id else None)(
            self.manager_target()), timeout=120)

    def release(self, message, signed):
        # Like run.py, but an air-gapped site reports only on trips: no waiting
        # for a connected Operator.
        waits = self.releases
        self.releases = []
        try:
            return super().release(message, signed)
        finally:
            self.releases = waits + self.releases

    # ── Attacks ──────────────────────────────────────────────────────────

    def evil_chart(self):
        """The manager's chart with the customer's releaseSigning ignored and a marker added."""
        sources = json.loads(run(["curl", "-sf", "-H", f"Authorization: Bearer {self.onboard['token']}",
                                  f"http://localhost:{MANAGER_PORT}/v1/deployments/{self.onboard['deploymentId']}"
                                  f"/bundle-sources?releaseId={self.releases[-1]['id']}"]).stdout)
        reference = sources["chart"]["reference"].removeprefix("oci://")
        host, _, rest = reference.partition("/")
        repository, separator, version = re.split(r"([:@])", rest, maxsplit=1)
        pull = f"oci://localhost:{MANAGER_PORT}/{rest}"
        work = self.work / "evil-chart"
        shutil.rmtree(work, ignore_errors=True)
        work.mkdir()
        run(["helm", "registry", "login", f"localhost:{MANAGER_PORT}", "--insecure",
             "--username", "admin", "--password-stdin"], input=self.admin_token)
        run(["helm", "pull", pull, "--plain-http", "--destination", work])
        archive = next(work.glob("*.tgz"))
        unpacked = work / "unpacked"
        unpacked.mkdir()
        run(["tar", "xzf", archive, "-C", unpacked])
        archive.unlink()
        changed = 0
        for template in unpacked.rglob("templates/*"):
            text = template.read_text()
            if ".Values.releaseSigning" not in text:
                continue
            text = re.sub(r"^(\s*)(\{\{-\s*\$releaseSigning := .*\}\})$",
                          lambda m: f'{m.group(1)}{m.group(2)}\n{m.group(1)}- name: EVIL_CHART\n'
                                    f'{m.group(1)}  value: "1"',
                          text, flags=re.M)
            text = text.replace(".Values.releaseSigning", ".Values.ignoredReleaseSigning")
            template.write_text(text)
            changed += 1
        assert changed, "no template reads releaseSigning"
        chart_dir = next(p.parent for p in unpacked.rglob("Chart.yaml"))
        run(["helm", "package", chart_dir, "--destination", work])
        evil = next(work.glob("*.tgz"))
        run(["helm", "push", evil, f"oci://{SITE_REGISTRY_HOST}/evil", "--plain-http"])
        name = repository.rsplit("/", 1)[-1]
        tag = version if separator == ":" else run(
            ["helm", "show", "chart", evil]).stdout.split("version:")[1].split()[0]
        headers = work / "headers"
        manifest = run(["curl", "-sf", "-D", headers, "-H", "Accept: application/vnd.oci.image.manifest.v1+json",
                        f"http://{SITE_REGISTRY_HOST}/v2/evil/{name}/manifests/{tag}"]).stdout
        digest = re.search(r"^docker-content-digest: (\S+)", headers.read_text().lower(), re.M).group(1)
        blobs = {}
        for descriptor in [json.loads(manifest)["config"], *json.loads(manifest)["layers"]]:
            body = subprocess.run(["curl", "-sf", f"http://{SITE_REGISTRY_HOST}/v2/evil/{name}/blobs/"
                                   f"{descriptor['digest']}"], capture_output=True, check=True).stdout
            blobs[descriptor["digest"]] = base64.b64encode(body).decode()
        attack = {"manifests": {repository: {"mediaType": "application/vnd.oci.image.manifest.v1+json",
                                             "body": manifest}},
                  "blobs": blobs}
        if separator == "@":
            attack["http"] = [{"path": r"/bundle-sources$", "ops": [
                {"op": "set", "path": ["chart", "reference"], "value": f"oci://{host}/{repository}@{digest}"}]}]
        return attack

    def attacks(self):
        target_path = r"^/v1/deployments/[^/]+/target$"
        evil_command = ["sh", "-c", "echo started: evil; exec sleep 2147483647"]

        def target_ops(*ops):
            return {"http": [{"path": target_path, "ops": list(ops)}]}

        app = ["releaseInfo", "stack", "resources", "app", "config"]
        evil_tool = b"#!/bin/sh\necho evil alien-deploy\n"
        return [
            {
                "name": "unsigned-target",
                "summary": "the manager drops the release statement and changes the command",
                **target_ops({"op": "delete", "path": ["releaseInfo", "signed"]},
                             {"op": "set", "path": [*app, "command"], "value": evil_command}),
                "refusal": r"refused the target.*(not signed|no signed release)",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "modified-stack-command",
                "summary": "the manager changes the release's command in the bundle's target",
                **target_ops({"op": "set", "path": [*app, "command"], "value": evil_command}),
                "refusal": r"refused the target.*differs from the signed release",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "older-signed-release",
                "summary": "the manager puts the older signed release-1 in a new bundle",
                "replace_target": True,
                "refusal": r"refused the target.*older than the applied",
                "took_effect": lambda: self.app_message() == "release-1",
            },
            {
                "name": "manager-env-vars",
                "summary": "the manager sets LD_PRELOAD in the bundle's target",
                "env_attack": True,
                "refusal": r"refused the target.*environment variables.*LD_PRELOAD",
                "took_effect": lambda: "LD_PRELOAD" in self.app_env(),
            },
            {
                "name": "evil-chart",
                "summary": "the registry serves another chart: it ignores releaseSigning and adds EVIL_CHART",
                "chart_attack": True,
                "new_release": "release-3",
                "refusal": None,
                "took_effect": lambda: "EVIL_CHART" in self.operator_env(),
            },
            {
                "name": "bundled-alien-deploy",
                "summary": "the manager puts its own alien-deploy build into the bundle",
                "http": [{"path": r"/bundle-sources$", "ops": [{"op": "set", "path": ["deployCli"], "value": [{
                    "platform": "linux-x86_64",
                    "url": f"http://localhost:{PROXY_PORT}/evil/alien-deploy",
                    "sha256": hashlib.sha256(evil_tool).hexdigest()}]}]}],
                "files": {"/evil/alien-deploy": base64.b64encode(evil_tool).decode()},
                "new_release": "release-4",
                "refusal": None,
                "took_effect": lambda: any(p.read_bytes() == evil_tool
                                           for p in (self.folder() / "tools").glob("alien-deploy*")),
            },
        ]

    def app_env(self):
        workload = self.app_workload()
        return {e["name"]: e.get("value") for c in workload["spec"]["template"]["spec"]["containers"]
                for e in c.get("env", [])}

    def run_trip_attack(self, attack):
        name = attack["name"]
        log(f"attack {name}: {attack['summary']}")
        if attack.get("new_release"):
            self.released(attack["new_release"])
        target = self.manager_target()
        loaded = {key: attack[key] for key in ("http", "manifests", "blobs", "files") if key in attack}
        if attack.get("replace_target"):
            loaded["http"] = [{"path": r"^/v1/deployments/[^/]+/target$", "ops": [
                {"op": "set", "path": [key], "value": value} for key, value in self.first_target.items()]}]
        if attack.get("env_attack"):
            variables = copy.deepcopy(target["config"]["environmentVariables"]["variables"])
            variables.append({"name": "LD_PRELOAD", "value": "/tmp/evil.so", "type": "plain",
                              "targetResources": ["app"]})
            loaded["http"] = [{"path": r"^/v1/deployments/[^/]+/target$", "ops": [
                {"op": "set", "path": ["config", "environmentVariables", "variables"], "value": variables}]}]
        if attack.get("chart_attack"):
            loaded.update(self.evil_chart())
        before = self.snapshot()
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.trip_log = []
        self.load_attack({"name": name, **loaded})
        result = {"attack": name, "summary": attack["summary"]}
        try:
            self.online()
        finally:
            self.clear_attack()
        code, output = self.site(check=False)
        refusal = None
        if attack.get("refusal"):
            refusal = next((line for line in self.operator_logs(since).splitlines()
                            if re.search(attack["refusal"], line)), None)
        time.sleep(10)
        effect = bool(attack["took_effect"]())
        after = self.snapshot()
        # alien-deploy hands each bundle over in these Secrets; they change on every trip.
        changed = sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k)
                         and not re.search(r"^Secret/.*-airgap-(target|status)$", k))
        expected_change = bool(attack.get("new_release"))
        if self.protected:
            passed = (not effect and (refusal is not None or not attack.get("refusal"))
                      and (not changed or expected_change))
        else:
            passed = effect
        result.update(site_exit=code, refusal=refusal, took_effect=effect, changed=changed, passed=passed)
        if expected_change:
            result["new_release_running"] = self.app_message() == attack["new_release"]
            passed = result["passed"] = passed and result["new_release_running"]
        self.save(f"attacks/{name}/operator.log", self.operator_logs(since))
        self.save(f"attacks/{name}/alien-deploy.json", self.trip_log)
        self.save(f"attacks/{name}/before.json", before)
        self.save(f"attacks/{name}/after.json", after)
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if passed else 'FAIL'} (took effect: {effect}, site exit: {code}, "
            f"changed: {changed})")
        self.results.append(result)
        self.online()
        if not self.protected and effect:
            self.restore_site()
        return result

    def restore_site(self):
        self.restores = getattr(self, "restores", 0) + 1
        message = f"restore-{self.restores}"
        self.released(message)
        self.trip_log = []
        self.trip(check=False)
        self.wait_for_message_only(message)

    def wait_for_message_only(self, message, timeout=240):
        wait_until(f"the app to run {message}", lambda: self.app_message() == message, timeout=timeout)

    def run_rollback(self):
        """`alien-deploy rollback` to the release before."""
        name = "site-rollback"
        log(f"attack {name}: the site admin runs alien-deploy rollback")
        before_message = self.app_message()
        self.trip_log = []
        code, output = self.deploy_cli("site", "rollback", "--folder", self.folder(),
                                       timeout=SITE_SYNC_TIMEOUT, check=False)
        time.sleep(10)
        effect = self.app_message() != before_message
        refused = code != 0 and "release the previous version again" in output
        result = {"attack": name, "summary": "alien-deploy rollback to the previous install",
                  "site_exit": code, "took_effect": effect, "refusal": output.strip().splitlines()[-1:],
                  "passed": (not effect and refused) if self.protected else effect}
        self.save(f"attacks/{name}/alien-deploy.json", self.trip_log)
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if result['passed'] else 'FAIL'} (took effect: {effect}, exit {code})")
        self.results.append(result)
        self.online()

    def suite(self):
        self.trip_log = []
        self.release("release-1", signed=True)
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.first_install()
        self.first_target = self.manager_target()
        self.check_multi_platform_image(since)
        self.wait_for_message_only("release-1")
        self.save("first-install.json", self.trip_log)

        self.trip_log = []
        self.released("release-2")
        self.trip()
        self.wait_for_message_only("release-2")
        self.save("normal-release.json", self.trip_log)
        self.results.append({"attack": "normal-release", "summary": "a new signed release, no admin action",
                             "took_effect": True, "passed": True})

        for attack in self.attacks():
            self.run_trip_attack(attack)

        self.run_rollback()

        message = "release-1"
        self.trip_log = []
        self.released(message)  # the old code again, with a new sequence
        self.trip(check=False)
        self.wait_for_message_only(message)
        self.save("rollback-by-re-release.json", self.trip_log)
        self.results.append({"attack": "rollback-by-re-release", "summary": "release-1's code released again",
                             "took_effect": True, "passed": True})

        self.save("results.json", self.results)
        failed = [r["attack"] for r in self.results if not r["passed"]]
        log("ALL PASSED" if not failed else f"FAILED: {failed}")
        return not failed


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", choices=["protected", "unprotected"], default="protected")
    parser.add_argument("--evidence", default=None, help="evidence directory (default: a new temporary one)")
    parser.add_argument("--alien", help="alien CLI binary (default: <binaries>/alien)")
    parser.add_argument("--alien-deploy", help="alien-deploy binary (default: <binaries>/alien-deploy)")
    parser.add_argument("--binaries", help="directory with the debug binaries (default: target/debug)")
    parser.add_argument("--down", action="store_true", help="remove everything the suite created")
    args = parser.parse_args()
    if args.evidence is None:
        import tempfile
        args.evidence = tempfile.mkdtemp(prefix="sign-e2e-airgap-")
    suite = AirgapSuite(args)
    if args.down:
        suite.down()
        return 0
    log(f"mode {args.mode}, evidence in {suite.evidence}")
    suite.up()
    return 0 if suite.suite() else 1


if __name__ == "__main__":
    sys.exit(main())
