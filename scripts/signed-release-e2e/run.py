#!/usr/bin/env python3
"""End-to-end attack suite for protected mode (signed releases).

Runs a real `alien-manager`, a real Operator in a kind cluster, and a real
signed `alien release` of a small app. A proxy between the Operator and the
manager plays a fully compromised control plane: it rewrites what the manager
sends, one attack at a time. For each attack the suite checks what reached
the cluster.

    python3 scripts/signed-release-e2e/run.py --mode protected
    python3 scripts/signed-release-e2e/run.py --mode unprotected

`protected` installs the Operator with the release key pinned and expects
every attack to be refused with nothing in the cluster changed. `unprotected`
publishes the same signed releases but installs the Operator without the key,
and expects every attack to take effect: that is what shows the attacks are
real. Either way, a normal signed release and a
rollback by re-release must still roll out.

In protected mode the suite first installs without the key, lets the manager
start a dynamic container, and then upgrades the installation to protected
mode: the container must be removed. `--only protection-turned-on` runs just
that step, for example against an older build with `--binaries`.

Needs docker, kind, kubectl and helm on PATH, and debug builds of `alien`,
`alien-manager` and `alien-operator` (`cargo build -p alien-cli -p
alien-manager -p alien-operator`), from target/debug or --binaries. Evidence (logs, snapshots, the summary)
goes to --evidence. Everything it creates is named `sign-e2e*`; `--down`
removes it.
"""

import argparse
import base64

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
import copy
import hashlib
import json
import os
import re
import secrets
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
CLUSTER = "sign-e2e"
CONTEXT = f"kind-{CLUSTER}"
NETWORK = "kind"
MANAGER = "sign-e2e-manager"
PROXY = "sign-e2e-proxy"
MANAGER_PORT = 18080
PROXY_PORT = 18081
# Deployments reach the manager only through the proxy.
PUBLIC_URL = f"http://{PROXY}:8080"
OPERATOR_IMAGE = "sign-e2e/alien-operator:dev"
# A plain registry the vendor's Operator image is published to. The manager
# passes Operator pulls through to it, as it does to the public one; nodes
# reach it as localhost:15001. (kind-loaded images don't work here: a kubelet
# that records image pulls verifies images it never pulled by pulling them.)
REGISTRY = "sign-e2e-registry"
REGISTRY_PORT = 15001
REGISTRY_HOST = f"localhost:{REGISTRY_PORT}"
UPSTREAM_OPERATOR = f"{REGISTRY}:5000/upstream/alien-operator:dev"
MANAGER_IMAGE = "sign-e2e/alien-manager:dev"
PROXY_IMAGE = "python:3.12-alpine"
NAMESPACE = "customer"
RELEASE_NAME = "signed-e2e"
CUSTOMER_DATABASE_URL = "postgres://db.customer.internal/app"


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


def run(args, check=True, capture=True, env=None, cwd=None, input=None):
    result = subprocess.run(
        [str(a) for a in args],
        check=False,
        text=True,
        capture_output=capture,
        env=env,
        cwd=cwd,
        input=input,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"{' '.join(map(str, args))} failed ({result.returncode}):\n"
            f"{result.stdout or ''}{result.stderr or ''}"
        )
    return result


def kubectl(*args, check=True):
    return run(["kubectl", "--context", CONTEXT, *args], check=check)


def wait_until(what, predicate, timeout=180, interval=3):
    deadline = time.time() + timeout
    while True:
        value = predicate()
        if value:
            return value
        if time.time() > deadline:
            raise TimeoutError(f"timed out after {timeout}s waiting for {what}")
        time.sleep(interval)


class Suite:
    def __init__(self, args):
        self.mode = args.mode
        self.protected = args.mode == "protected"
        self.evidence = Path(args.evidence).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = self.evidence / "work"
        if self.work.exists():
            shutil.rmtree(self.work)
        self.work.mkdir()
        self.attack_dir = self.work / "attack"
        self.attack_dir.mkdir()
        self.app = self.work / "app"
        shutil.copytree(HERE / "app", self.app)
        self.binaries = Path(args.binaries or REPO / "target/debug").resolve()
        self.alien = Path(args.alien or self.binaries / "alien")
        self.admin_token = "ax_admin_" + secrets.token_hex(16)
        self.results = []
        self.releases = []
        self.targets = "linux-x64"
        # The Operator image the manager's charts run, through the manager.
        self.manager_operator_image = UPSTREAM_OPERATOR

    # ── Environment ──────────────────────────────────────────────────────

    def cli_env(self):
        env = dict(os.environ)
        env.update(
            {
                "ALIEN_MANAGER_URL": f"http://localhost:{MANAGER_PORT}",
                "ALIEN_API_KEY": self.admin_token,
                "XDG_CONFIG_HOME": str(self.work / "config"),
                "ALIEN_SIGNING_HOME": str(self.work / "signing"),
                "NO_COLOR": "1",
            }
        )
        return env

    def alien_cli(self, *args, check=True):
        return run([self.alien, *args], env=self.cli_env(), cwd=self.app, check=check)

    def up(self):
        if CLUSTER not in run(["kind", "get", "clusters"]).stdout.split():
            log("creating the kind cluster")
            run(["kind", "create", "cluster", "--name", CLUSTER,
                 "--config", HERE / "kind.yaml", "--wait", "180s"])
        for node in run(["kind", "get", "nodes", "--name", CLUSTER]).stdout.split():
            host = f"{PROXY}:8080"
            run(["docker", "exec", node, "mkdir", "-p", f"/etc/containerd/certs.d/{host}"])
            run(["docker", "exec", "-i", node, "tee", f"/etc/containerd/certs.d/{host}/hosts.toml"],
                input=f'[host."http://{host}"]\n  capabilities = ["pull", "resolve"]\n')

        self.build_images()
        run(["docker", "rm", "-f", REGISTRY], check=False)
        run(["docker", "run", "-d", "--name", REGISTRY, "--network", NETWORK,
             "-p", f"127.0.0.1:{REGISTRY_PORT}:5000", "registry:2"])
        for node in run(["kind", "get", "nodes", "--name", CLUSTER]).stdout.split():
            run(["docker", "exec", node, "mkdir", "-p", f"/etc/containerd/certs.d/{REGISTRY_HOST}"])
            run(["docker", "exec", "-i", node, "tee", f"/etc/containerd/certs.d/{REGISTRY_HOST}/hosts.toml"],
                input=f'[host."http://{REGISTRY}:5000"]\n  capabilities = ["pull", "resolve"]\n')
        self.publish(OPERATOR_IMAGE, "upstream/alien-operator:dev")

        for name in (MANAGER, PROXY):
            run(["docker", "rm", "-f", name], check=False)
        data = self.work / "manager-data"
        data.mkdir()
        run(["docker", "run", "-d", "--name", MANAGER, "--network", NETWORK,
             "--user", f"{os.getuid()}:{os.getgid()}",
             "-p", f"127.0.0.1:{MANAGER_PORT}:8080", "-v", f"{data}:/data",
             "-e", "STATE_DIR=/data", "-e", f"BASE_URL={PUBLIC_URL}",
             "-e", f"ALIEN_ADMIN_TOKEN={self.admin_token}",
             "-e", "RUST_LOG=info",
             "-e", f'ALIEN_MANAGER_CONFIG=[operator]\nimage = "{self.manager_operator_image}"\n'
                   'insecure-registry = true\n',
             MANAGER_IMAGE])
        run(["docker", "run", "-d", "--name", PROXY, "--network", NETWORK,
             "--user", f"{os.getuid()}:{os.getgid()}", "-p", f"127.0.0.1:{PROXY_PORT}:8080",
             "-v", f"{HERE / 'proxy.py'}:/proxy.py:ro", "-v", f"{self.attack_dir}:/attack",
             "-e", f"UPSTREAM=http://{MANAGER}:8080",
             PROXY_IMAGE, "python3", "-u", "/proxy.py"])
        wait_until("the manager", lambda: run(
            ["curl", "-sf", f"http://localhost:{MANAGER_PORT}/health"], check=False).returncode == 0)

        # The developer sets up signing: a signer list with this machine's key,
        # a CI key in a file and a recovery key. Releases are signed with the CI key.
        self.key_file = self.work / "ci.pem"
        init = json.loads(self.alien_cli(
            "signing", "init", "--backend", "software", "--ci-key-out", self.key_file,
            "--recovery-key-out", self.work / "recovery.pem", "--json").stdout)
        self.fingerprint = init["fingerprint"]
        # The key that signs Operator builds; the suite never signs one.
        self.components_key = "ecdsa-p256:" + base64.b64encode(
            ec.generate_private_key(ec.SECP256R1()).public_key().public_bytes(
                serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)).decode()
        log(f"signing fingerprint {self.fingerprint}")

    def build_images(self):
        build = self.work / "images"
        for binary, image, state in (
            ("alien-operator", OPERATOR_IMAGE, "/var/lib/operator"),
            ("alien-manager", MANAGER_IMAGE, "/data"),
        ):
            context = build / binary
            context.mkdir(parents=True)
            run(["strip", "-o", context / binary, self.binaries / binary])
            (context / "Dockerfile").write_text(
                "FROM debian:bookworm-slim\n"
                "RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates"
                f" && rm -rf /var/lib/apt/lists/* && mkdir -p {state} && chown 65532:65532 {state}\n"
                f"COPY {binary} /app/{binary}\n"
                # Debug builds keep large futures on the stack.
                "ENV RUST_MIN_STACK=67108864\n"
                "USER 65532\n"
                f'ENTRYPOINT ["/app/{binary}"]\n'
            )
            run(["docker", "build", "-q", "-t", image, context])
        shutil.rmtree(build)

    def publish(self, image, name):
        """Push a local image to the registry; return how nodes pull it."""
        reference = f"{REGISTRY_HOST}/{name}"
        run(["docker", "tag", image, reference])
        run(["docker", "push", "-q", reference])
        return reference

    def down(self):
        for name in (MANAGER, PROXY, REGISTRY):
            run(["docker", "rm", "-f", name], check=False)
        run(["kind", "delete", "cluster", "--name", CLUSTER], check=False)


    # ── Vendor and installer actions ─────────────────────────────────────

    def release(self, message, signed):
        """`alien release` of the app printing `message`, signed or not."""
        # The manager rolls a release out only to deployments in these statuses
        # (an Operator restarting after a self-update isn't one, for a moment).
        if self.releases:
            kubectl("-n", NAMESPACE, "rollout", "status", f"deployment/{RELEASE_NAME}", "--timeout=180s")
            wait_until("the deployment to take a new release", lambda: all(
                d["status"] in ("running", "update-failed", "refresh-failed")
                for d in self.manager_get("/v1/deployments").get("items", [])), timeout=180)
            time.sleep(35)  # one more sync, so the manager's status is current
        config_path = self.app / "alien.json"
        config = json.loads(config_path.read_text())
        config["resources"]["app"]["config"]["code"]["toolchain"]["buildArgs"] = {"MESSAGE": message}
        config_path.write_text(json.dumps(config, indent=2) + "\n")
        args = ["release", "--platforms", "kubernetes", "--targets", self.targets, "--no-git", "--json"]
        if signed:
            args += ["--sign", "--signing-key-file", self.key_file, "--signing-fingerprint", self.fingerprint]
        output = json.loads(self.alien_cli(*args).stdout)
        release = self.manager_get(f"/v1/releases/{output['releaseId']}")
        image = release["stack"]["kubernetes"]["resources"]["app"]["config"]["code"]["image"]
        self.releases.append({"message": message, "signed": signed, "id": output["releaseId"], "image": image})
        log(f"released {message} ({'signed' if signed else 'unsigned'}) as {output['releaseId']}: {image}")
        return release

    def deployment_id(self):
        """The customer's deployment, once the installation registered it."""
        items = self.manager_get("/v1/deployments").get("items", [])
        return items[0]["id"] if items else "dep_customer"

    def manager_post(self, path, body):
        result = run(["curl", "-sf", "-X", "POST", "-H", f"Authorization: Bearer {self.admin_token}",
                      "-H", "content-type: application/json", "--data-binary", "@-",
                      f"http://localhost:{MANAGER_PORT}{path}"], input=json.dumps(body))
        return result.stdout

    def signed_config(self, inputs):
        """A config/v1 for the customer's deployment, signed with the CI key the
        way `alien deployments set-inputs --sign` signs it."""
        key = serialization.load_pem_private_key(self.key_file.read_bytes(), password=None)
        release = json.loads(base64.b64decode(self.manager_get(
            f"/v1/releases/{self.releases[-1]['id']}")["signedStacks"]["kubernetes"]["payload"]))
        envelope = {"type": "config/v1", "project": release["project"], "deployment": self.deployment_id(),
                    "sequence": int(time.time() * 1000), "signerListVersion": release["signerListVersion"],
                    "body": {"inputs": inputs}}
        payload = json.dumps(envelope).encode()
        sig = key.sign(payload, ec.ECDSA(hashes.SHA256()))
        return {"payload": base64.b64encode(payload).decode(),
                "signatures": [{"signer": "ci", "sig": base64.b64encode(sig).decode()}]}

    def manager_get(self, path):
        result = run(["curl", "-sf", "-H", f"Authorization: Bearer {self.admin_token}",
                      f"http://localhost:{MANAGER_PORT}{path}"])
        return json.loads(result.stdout)

    def install(self, protected=None):
        kubectl("delete", "namespace", NAMESPACE, "--ignore-not-found", "--wait=true", "--timeout=180s")
        onboard = json.loads(self.alien_cli("onboard", "customer", "--platforms", "kubernetes", "--json").stdout)
        self.chart = onboard["helm"]["chart"].replace(PUBLIC_URL.removeprefix("http://"), f"localhost:{MANAGER_PORT}")
        self.management_token = onboard["token"]
        run(["helm", "registry", "login", f"localhost:{MANAGER_PORT}", "--insecure",
             "--username", "customer", "--password-stdin"], input=onboard["token"])
        self.helm("install", self.protected if protected is None else protected)

    def helm(self, command, protected):
        """`helm install` or `helm upgrade` with the customer's values."""
        values = {
            "management": {"token": self.management_token, "name": "customer"},
            "inputValues": {"databaseUrl": CUSTOMER_DATABASE_URL},
            "tunnel": {"enabled": False},
        }
        if protected:
            # The anchor the customer's setup pins, from the vendor's install
            # guide. It names the deployment the installation registered as, so
            # config signed for this deployment verifies and config signed for
            # another doesn't.
            values["releaseSigning"] = {
                "protected": True, "project": "default", "deployment": self.deployment_id(),
                "signing": self.fingerprint, "components": self.components_key,
            }
        values_file = self.work / "values.yaml"
        values_file.write_text(json.dumps(values))
        (self.evidence / "customer-values.json").write_text(json.dumps(
            {**values, "management": {"name": "customer", "token": "<redacted>"}}, indent=2))
        run(["helm", command, RELEASE_NAME, self.chart, "--plain-http", "--kube-context", CONTEXT,
             "--namespace", NAMESPACE, "--create-namespace", "--values", values_file,
             "--wait", "--timeout", "5m"])
        # The customer opts in to dynamic containers; protected mode must still refuse them.
        kubectl("-n", NAMESPACE, "set", "env", f"deployment/{RELEASE_NAME}", "OPERATOR_DYNAMIC_CONTAINERS=true")
        kubectl("-n", NAMESPACE, "rollout", "status", f"deployment/{RELEASE_NAME}", "--timeout=180s")
        self.installed_operator_image = self.operator_workload()["spec"]["template"]["spec"]["containers"][0]["image"]

    # ── Observing the cluster ────────────────────────────────────────────

    def operator_logs(self, since):
        result = kubectl("-n", NAMESPACE, "logs", f"deployment/{RELEASE_NAME}", "--since-time", since,
                         "--timestamps", check=False)
        return re.sub(r"\x1b\[[0-9;]*m", "", result.stdout)

    def app_workload(self):
        result = kubectl("-n", NAMESPACE, "get", "deployments", "-l", "alien.dev/resource-id=app",
                         "-o", "json", check=False)
        items = json.loads(result.stdout or "{}").get("items", []) if result.returncode == 0 else []
        if not items:
            result = kubectl("-n", NAMESPACE, "get", "deployments", "-o", "json")
            items = [d for d in json.loads(result.stdout)["items"] if d["metadata"]["name"] != RELEASE_NAME]
        return items[0] if items else None

    def app_message(self):
        """What the running app pod printed at start, or None."""
        workload = self.app_workload()
        if not workload:
            return None
        selector = ",".join(f"{k}={v}" for k, v in workload["spec"]["selector"]["matchLabels"].items())
        pods = json.loads(kubectl("-n", NAMESPACE, "get", "pods", "-l", selector, "-o", "json").stdout)["items"]
        running = [p for p in pods if p["status"].get("phase") == "Running" and not p["metadata"].get("deletionTimestamp")]
        if len(running) != 1:
            return None
        logs = kubectl("-n", NAMESPACE, "logs", running[0]["metadata"]["name"], check=False).stdout
        match = re.search(r"^started: (.*)$", logs, re.M)
        return match and match.group(1)

    def wait_for_message(self, message, timeout=300):
        wait_until(f"the app to run {message}", lambda: self.app_message() == message, timeout=timeout)
        # The manager rolls a new release out only to deployments it sees running.
        release_id = next(r["id"] for r in reversed(self.releases) if r["message"] == message)
        wait_until(f"the manager to see {message} running", lambda: any(
            d["status"] == "running" and d.get("currentReleaseId") == release_id
            for d in self.manager_get("/v1/deployments").get("items", [])), timeout=timeout)

    def snapshot(self):
        """What runs in the namespace and what it is given, without status."""
        kinds = "deployments,statefulsets,daemonsets,jobs,cronjobs,pods,services,secrets,configmaps,serviceaccounts,roles,rolebindings"
        items = json.loads(kubectl("-n", NAMESPACE, "get", kinds, "-o", "json").stdout)["items"]
        snapshot = {}
        for item in items:
            meta = item["metadata"]
            kind = item["kind"]
            if kind == "ConfigMap" and meta["name"] == "kube-root-ca.crt":
                continue
            if kind == "Pod" and meta.get("deletionTimestamp"):
                continue
            key = f"{kind}/{meta['name']}"
            if kind in ("Secret", "ConfigMap"):
                data = json.dumps(item.get("data", {}), sort_keys=True)
                snapshot[key] = {"data_sha256": hashlib.sha256(data.encode()).hexdigest()}
            elif kind == "Pod":
                snapshot[key] = {
                    "uid": meta["uid"],
                    "containers": [{k: c.get(k) for k in ("image", "command", "args", "env")}
                                   for c in item["spec"]["containers"]],
                }
            else:
                spec = copy.deepcopy(item.get("spec", {}))
                snapshot[key] = {"uid": meta["uid"], "generation": meta.get("generation"),
                                 "spec_sha256": hashlib.sha256(json.dumps(spec, sort_keys=True).encode()).hexdigest()}
                if kind == "Deployment":
                    snapshot[key]["containers"] = [
                        {k: c.get(k) for k in ("image", "command", "args", "env")}
                        for c in item["spec"]["template"]["spec"]["containers"]]
        return snapshot

    def save(self, name, value):
        path = self.evidence / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value if isinstance(value, str) else json.dumps(value, indent=2, sort_keys=True))

    # ── The compromised control plane ────────────────────────────────────

    def captured_target(self):
        return json.loads((self.attack_dir / "capture/target.json").read_text())

    def load_attack(self, attack):
        (self.attack_dir / "attack.json.tmp").write_text(json.dumps(attack))
        os.replace(self.attack_dir / "attack.json.tmp", self.attack_dir / "attack.json")

    def clear_attack(self):
        self.load_attack({})

    def push_evil_image(self):
        """An image the vendor never released, in the vendor's own registry."""
        context = self.work / "evil"
        context.mkdir(exist_ok=True)
        (context / "Dockerfile").write_text(
            'FROM busybox:1.37\nCMD ["sh", "-c", "echo started: evil; exec sleep 2147483647"]\n')
        tag = f"localhost:{MANAGER_PORT}/artifacts/default:evil"
        env = {**os.environ, "DOCKER_CONFIG": str(self.work / "docker")}
        run(["docker", "build", "-q", "-t", tag, context])
        run(["docker", "login", f"localhost:{MANAGER_PORT}", "-u", "admin", "--password-stdin"],
            input=self.admin_token, env=env)
        run(["docker", "push", tag], env=env)
        manifest = run(["curl", "-sf", "-u", f"admin:{self.admin_token}",
                        "-H", "Accept: application/vnd.oci.image.manifest.v1+json,"
                        "application/vnd.docker.distribution.manifest.v2+json",
                        "-D", self.work / "evil-headers",
                        f"http://localhost:{MANAGER_PORT}/v2/artifacts/default/manifests/evil"]).stdout
        headers = (self.work / "evil-headers").read_text().lower()
        media_type = re.search(r"^content-type: (\S+)", headers, re.M).group(1)
        self.evil_manifest = {"mediaType": media_type, "body": manifest}

    def evil_operator_image(self):
        return self.publish(OPERATOR_IMAGE, "attacker/alien-operator:evil")

    def attacks(self):
        """Every attack: what the control plane sends, and how to see it took effect."""
        target = self.baseline_target
        evil_command = ["sh", "-c", "echo started: evil; exec sleep 2147483647"]

        def with_stack(change):
            modified = copy.deepcopy(target)
            change(modified["releaseInfo"]["stack"]["resources"]["app"]["config"])
            return [{"op": "set", "path": ["target"], "value": modified}]

        def app_env():
            workload = self.app_workload()
            return {e["name"]: e.get("value") for c in workload["spec"]["template"]["spec"]["containers"]
                    for e in c.get("env", [])}

        def tampered_payload():
            modified = copy.deepcopy(target)
            payload = json.loads(base64.b64decode(modified["releaseInfo"]["signed"]["payload"]))
            payload["body"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            payload["body"]["workloads"][0]["command"] = evil_command
            modified["releaseInfo"]["signed"]["payload"] = base64.b64encode(
                json.dumps(payload).encode()).decode()
            modified["releaseInfo"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            return [{"op": "set", "path": ["target"], "value": modified}]

        def config_change(variables, input_values):
            modified = copy.deepcopy(target)
            config = modified["config"]
            existing = {v["name"]: v for v in config["environmentVariables"]["variables"]}
            for name, value in variables.items():
                existing[name] = {"name": name, "value": value, "type": "plain", "targetResources": ["app"]}
            config["environmentVariables"]["variables"] = list(existing.values())
            config.setdefault("inputValues", {}).update(input_values)
            return [{"op": "set", "path": ["target"], "value": modified}]

        def attacker_signed():
            """A signer list version 2 that adds the attacker's key, and a release
            the attacker signs with it: what a control plane that forged the list
            would send."""
            attacker = ec.generate_private_key(ec.SECP256R1())
            attacker_key = "ecdsa-p256:" + base64.b64encode(attacker.public_key().public_bytes(
                serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)).decode()
            modified = copy.deepcopy(target)
            lists = modified["releaseInfo"]["signerLists"]
            first = json.loads(base64.b64decode(lists[0]["payload"]))
            first_digest = "sha256:" + hashlib.sha256(base64.b64decode(lists[0]["payload"])).hexdigest()
            body = first["body"]
            body["previous"] = first_digest
            body["signers"].append({"id": "attacker", "key": attacker_key, "kind": "ci", "roles": ["release", "admin"]})
            forged = {**first, "sequence": 2, "signerListVersion": 1, "body": body}
            def sign(document, signer):
                payload = json.dumps(document).encode()
                sig = attacker.sign(payload, ec.ECDSA(hashes.SHA256()))
                return {"payload": base64.b64encode(payload).decode(),
                        "signatures": [{"signer": signer, "sig": base64.b64encode(sig).decode()}]}
            admin = next(s["id"] for s in first["body"]["signers"] if "admin" in s["roles"] and s["kind"] != "recovery")
            modified["releaseInfo"]["signerLists"] = [lists[0], sign(forged, admin)]
            release = json.loads(base64.b64decode(modified["releaseInfo"]["signed"]["payload"]))
            release["sequence"] += 1000
            release["signerListVersion"] = 2
            release["body"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            release["body"]["workloads"][0]["command"] = evil_command
            modified["releaseInfo"]["signed"] = sign(release, "attacker")
            modified["releaseInfo"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            return [{"op": "set", "path": ["target"], "value": modified}]

        def unsigned():
            modified = copy.deepcopy(target)
            modified["releaseInfo"].pop("signed", None)
            modified["releaseInfo"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            return [{"op": "set", "path": ["target"], "value": modified}]

        evil_operator = self.evil_operator_image()
        attacks = [
            {
                "name": "forged-signer-list",
                "summary": "the manager forges signer list version 2 with its own key and signs a release with it",
                "sync": attacker_signed(),
                "refusal": r"refused the target.*TRUST_SIGNATURE_REFUSED|refused the target.*TRUST_SIGNER_LIST_REFUSED",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "unsigned-target",
                "summary": "the manager drops the signature and sends its own command",
                "sync": unsigned(),
                "refusal": r"refused the target.*not signed",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "modified-stack-image",
                "summary": "the manager changes the release's image and command",
                "sync": with_stack(lambda app: app.update(
                    code={"type": "image", "image": "busybox:1.37"}, command=evil_command)),
                "refusal": r"refused the target.*differs from the signed release",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "modified-stack-env",
                "summary": "the manager adds LD_PRELOAD to the release's container environment",
                "sync": with_stack(lambda app: app["environment"].update(LD_PRELOAD="/tmp/evil.so")),
                "refusal": r"refused the target.*differs from the signed release",
                "took_effect": lambda: "LD_PRELOAD" in app_env(),
            },
            {
                "name": "tampered-signed-bytes",
                "summary": "the manager edits the signed statement itself (same release, new bytes)",
                "sync": tampered_payload(),
                "refusal": r"refused the target.*TRUST_SIGNATURE_REFUSED",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "older-signed-release",
                "summary": "the manager replays the older signed release-1",
                "sync": [{"op": "set", "path": ["target"], "value": self.first_target}],
                "refusal": r"refused the target.*already applied",
                "took_effect": lambda: self.app_message() == "release-1",
            },
            {
                "name": "manager-env-vars",
                "summary": "the manager sets LD_PRELOAD and NODE_OPTIONS",
                "sync": config_change({"LD_PRELOAD": "/tmp/evil.so",
                                       "NODE_OPTIONS": "--require /tmp/evil.js"}, {}),
                "refusal": r"refused the target.*environment variables.*LD_PRELOAD",
                "took_effect": lambda: "LD_PRELOAD" in app_env() or "NODE_OPTIONS" in app_env(),
            },
            {
                "name": "redirected-deployer-input",
                "summary": "the manager points DATABASE_URL (a deployer input) at its own database",
                "sync": config_change({"DATABASE_URL": "postgres://attacker.example/steal"},
                                      {"databaseUrl": "postgres://attacker.example/steal"}),
                # Accepted: the value is taken from the installation, not the manager.
                "refusal": None,
                "took_effect": lambda: app_env().get("DATABASE_URL") == "postgres://attacker.example/steal",
            },
            {
                "name": "manager-operator-image",
                "summary": "the manager tells the Operator to update itself to an image no release names",
                "sync": [{"op": "set", "path": ["targetOperatorImage"], "value": evil_operator}],
                "refusal": r"refused to update the Operator to an image no accepted release signed",
                "took_effect": lambda: evil_operator in json.dumps(self.operator_workload()["spec"]["template"]),
            },
            {
                "name": "dynamic-container",
                "summary": "the manager asks for a dynamic container running its own image",
                "sync": [{"op": "set", "path": ["targetDynamicContainers"], "value": [{
                    "name": "evil", "generation": 1, "image": "busybox:1.37", "cpu": "50m",
                    "memory": "32Mi", "replicas": 1, "ports": [], "deleted": False, "env": {}}]}],
                "refusal": None,
                "settle": 75,
                "took_effect": lambda: bool(kubectl(
                    "-n", NAMESPACE, "get", "deployments", "-l", "alien.dev/dynamic-container=evil",
                    "-o", "name").stdout.strip()),
            },
        ]
        return attacks

    def operator_workload(self):
        return json.loads(kubectl("-n", NAMESPACE, "get", "deployment", RELEASE_NAME, "-o", "json").stdout)

    def run_attack(self, attack):
        name = attack["name"]
        log(f"attack {name}: {attack['summary']}")
        before = self.snapshot()
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.load_attack({"name": name, "sync": attack.get("sync", []), "manifests": attack.get("manifests", {})})
        result = {"attack": name, "summary": attack["summary"]}
        try:
            if attack.get("action"):
                attack["action"]()
            if self.protected:
                refusal = None
                if attack.get("refusal"):
                    refusal = wait_until(
                        f"the Operator to refuse {name}",
                        lambda: next((line for line in self.operator_logs(since).splitlines()
                                      if re.search(attack["refusal"], line)), None),
                        timeout=150)
                    time.sleep(10)
                else:
                    # Nothing to refuse: wait two syncs for an effect that must not come.
                    time.sleep(attack.get("settle", 75))
                effect = attack["took_effect"]()
                after = self.snapshot()
                changed = sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))
                result.update(refusal=refusal, took_effect=effect, changed=changed,
                              passed=(not effect and (not changed or attack.get("may_change"))
                                      and (refusal is not None or not attack.get("refusal"))))
            else:
                try:
                    wait_until(f"{name} to take effect", attack["took_effect"], timeout=attack.get("timeout", 180))
                    effect = True
                except TimeoutError:
                    effect = False
                after = self.snapshot()
                changed = sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))
                result.update(took_effect=effect, changed=changed, passed=effect)
        finally:
            self.clear_attack()
            logs = self.operator_logs(since)
            self.save(f"attacks/{name}/operator.log", logs)
            self.save(f"attacks/{name}/before.json", before)
            self.save(f"attacks/{name}/after.json", self.snapshot())
            self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if result['passed'] else 'FAIL'} "
            f"(took effect: {result['took_effect']}, changed: {result['changed']})")
        self.results.append(result)
        if not self.protected and result["took_effect"]:
            self.restore()
        return result

    def restore(self):
        """After an attack took effect, roll a fresh release out again."""
        self.restores = getattr(self, "restores", 0) + 1
        message = f"restore-{self.restores}"
        self.release(message, signed=True)
        kubectl("-n", NAMESPACE, "rollout", "undo", f"deployment/{RELEASE_NAME}", check=False) \
            if self.operator_image_changed() else None
        self.wait_for_message(message)
        self.baseline_target = wait_until("the manager's new target", lambda: self.target_for(message))

    def operator_image_changed(self):
        return any(c["image"] != self.installed_operator_image
                   for c in self.operator_workload()["spec"]["template"]["spec"]["containers"])

    def target_for(self, message):
        """The last target the manager sent, if it is the release printing `message`."""
        try:
            target = self.captured_target()
        except (FileNotFoundError, json.JSONDecodeError):
            return None
        wanted = next(r for r in reversed(self.releases) if r["message"] == message)
        return target if target["releaseInfo"].get("releaseId") == wanted["id"] else None

    # ── The suite ─────────────────────────────────────────────────────────

    def suite(self):
        # Releases are signed in both modes: only the installation differs.
        signed = True
        self.release("release-1", signed)
        if self.protected:
            # The customer turns protected mode on for an existing installation.
            self.install(protected=False)
            self.wait_for_message("release-1")
            self.run_protection_turned_on()
        else:
            self.install()
        self.wait_for_message("release-1")
        self.first_target = wait_until("the first target", lambda: self.target_for("release-1"))
        self.save("normal-deploy/target.json", self.first_target)
        self.save("normal-deploy/snapshot.json", self.snapshot())
        self.save("normal-deploy/app-workload.json", self.app_workload())

        log("normal release: release-2 rolls out with no installer action")
        self.release("release-2", signed)
        self.wait_for_message("release-2")
        self.baseline_target = wait_until("the release-2 target", lambda: self.target_for("release-2"))
        self.results.append({"attack": "normal-release", "summary": "a new signed release rolls out",
                             "passed": True, "took_effect": True, "changed": []})

        for attack in self.attacks():
            self.run_attack(attack)

        log("developer input: the manager changes a developer input")
        self.run_developer_input()
        self.run_registry_swap()
        self.run_unsigned_release()

        log("rollback: re-releasing release-1's code rolls out as a new release")
        self.release("release-1", signed)
        self.wait_for_message("release-1")
        self.results.append({"attack": "rollback-by-re-release",
                             "summary": "re-releasing release-1's code rolls it back",
                             "passed": True, "took_effect": True, "changed": []})
        self.save("operator-full.log", self.operator_logs("2000-01-01T00:00:00Z"))
        self.save("results.json", self.results)
        return all(r["passed"] for r in self.results)

    def dynamic_objects(self):
        """The dynamic containers' Deployments, Services and Secrets in the namespace."""
        return sorted(kubectl("-n", NAMESPACE, "get", "deployments,services,secrets",
                              "-l", "alien.dev/dynamic-deployment", "-o", "name").stdout.split())

    def run_protection_turned_on(self):
        """An unprotected installation runs a dynamic container the manager asked
        for. The customer then upgrades it to protected mode, which runs none:
        the container must be removed, although the manager keeps asking for it."""
        name = "protection-turned-on"
        log(f"attack {name}: a dynamic container started before protected mode was on")
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.load_attack({"name": name, "manifests": {}, "sync": [
            {"op": "set", "path": ["targetDynamicContainers"], "value": [{
                "name": "early", "generation": 1, "image": "busybox:1.37", "cpu": "50m",
                "memory": "32Mi", "replicas": 1, "ports": [], "deleted": False, "env": {},
                "secretEnv": {"TOKEN": "from-the-manager"}}]}]})
        try:
            started = wait_until("the unprotected Operator to start the dynamic container",
                                 lambda: [o for o in self.dynamic_objects() if o.startswith("deployment")]
                                 and self.dynamic_objects(), timeout=240)
            before = self.snapshot()
            log(f"running before protected mode: {started}")
            self.helm("upgrade", protected=True)
            try:
                wait_until("protected mode to remove the dynamic container",
                           lambda: not self.dynamic_objects(), timeout=240)
                removed = True
            except TimeoutError:
                removed = False
            left = self.dynamic_objects()
        finally:
            self.clear_attack()
        after = self.snapshot()
        result = {"attack": name, "summary": "protected mode removes a dynamic container started before it",
                  "started": started, "left": left, "took_effect": bool(left), "passed": removed,
                  "changed": sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))}
        self.save(f"attacks/{name}/operator.log", self.operator_logs(since))
        self.save(f"attacks/{name}/before.json", before)
        self.save(f"attacks/{name}/after.json", after)
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if removed else 'FAIL'} (left running: {left})")
        self.results.append(result)
        return result

    def run_developer_input(self):
        target = copy.deepcopy(self.baseline_target)
        target["config"].setdefault("inputValues", {})["logLevel"] = "debug"
        target["config"]["environmentVariables"]["variables"].append(
            {"name": "LOG_LEVEL", "value": "debug", "type": "plain", "targetResources": ["app"]})
        name = "manager-developer-input"
        log(f"attack {name}: the manager changes logLevel, a developer input")
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.load_attack({"name": name, "sync": [{"op": "set", "path": ["target"], "value": target}]})
        try:
            def level():
                workload = self.app_workload()
                return any(e["name"] == "LOG_LEVEL" and e.get("value") == "debug"
                           for c in workload["spec"]["template"]["spec"]["containers"] for e in c.get("env", []))
            try:
                wait_until("LOG_LEVEL=debug", level, timeout=180)
                effect = True
            except TimeoutError:
                effect = False
        finally:
            self.clear_attack()
        passed = effect if not self.protected else not effect
        result = {"attack": name, "took_effect": effect, "changed": [], "passed": passed,
                  "summary": "a protected deployment takes developer inputs only from signed config"}
        self.save(f"attacks/{name}/operator.log", self.operator_logs(since))
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if passed else 'FAIL'} (took effect: {effect})")
        self.results.append(result)
        self.restore()
        if self.protected:
            self.run_signed_developer_input()

    def run_signed_developer_input(self):
        """The developer signs a config/v1 setting logLevel; it reaches the workload."""
        name = "signed-developer-input"
        log(f"{name}: the developer signs logLevel=warn for this deployment")
        self.manager_post(f"/v1/deployments/{self.deployment_id()}/config-statements",
                          {"statement": self.signed_config({"logLevel": "warn"})})
        def level():
            workload = self.app_workload()
            return any(e["name"] == "LOG_LEVEL" and e.get("value") == "warn"
                       for c in workload["spec"]["template"]["spec"]["containers"] for e in c.get("env", []))
        try:
            wait_until("LOG_LEVEL=warn", level, timeout=240)
            effect = True
        except TimeoutError:
            effect = False
        result = {"attack": name, "took_effect": effect, "changed": [], "passed": effect,
                  "summary": "a signed developer input reaches the workload"}
        self.save(f"attacks/{name}/result.json", result)
        log(f"{name}: {'PASS' if effect else 'FAIL'} (took effect: {effect})")
        self.results.append(result)

    def run_registry_swap(self):
        """The registry serves other bytes for the release's image; the node has to pull again."""
        name = "registry-swap"
        log(f"attack {name}: the registry proxy serves an unreleased image for the app's reference")
        self.push_evil_image()
        if not self.protected:
            # Unsigned releases reference images by tag, as they always have.
            self.release("tagged", signed=False)
            self.wait_for_message("tagged")
        image = self.app_workload()["spec"]["template"]["spec"]["containers"][0]["image"]
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.load_attack({"name": name, "manifests": {"artifacts/default": self.evil_manifest}})
        try:
            for node in run(["kind", "get", "nodes", "--name", CLUSTER]).stdout.split():
                run(["docker", "exec", node, "crictl", "rmi", image], check=False)
            kubectl("-n", NAMESPACE, "delete", "pods", "-l",
                    ",".join(f"{k}={v}" for k, v in self.app_workload()["spec"]["selector"]["matchLabels"].items()),
                    "--wait=false")
            try:
                wait_until("the swapped image to run", lambda: self.app_message() == "evil", timeout=120)
                effect = True
            except TimeoutError:
                effect = False
            events = kubectl("-n", NAMESPACE, "get", "events", "--sort-by=.lastTimestamp").stdout
        finally:
            self.clear_attack()
        pull_refused = bool(re.search(r"(?i)digest|failed to pull|ErrImagePull", events))
        passed = effect if not self.protected else (not effect and pull_refused)
        result = {"attack": name, "took_effect": effect, "changed": ["the app pod was deleted to force a pull"],
                  "pull_refused": pull_refused, "passed": passed, "image": image,
                  "summary": "the registry serves another image's manifest for the release's image"}
        self.save(f"attacks/{name}/events.txt", events)
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if passed else 'FAIL'} (took effect: {effect}, pull refused: {pull_refused})")
        self.results.append(result)
        if effect or self.protected:
            # Protected: the pod waits for its image; it recovers once the registry is honest.
            if self.protected:
                kubectl("-n", NAMESPACE, "delete", "pods", "-l", ",".join(
                    f"{k}={v}" for k, v in self.app_workload()["spec"]["selector"]["matchLabels"].items()),
                    "--wait=false")
                self.wait_for_message(self.current_message())
            else:
                self.restore()

    def current_message(self):
        return self.releases[-1]["message"]

    def run_unsigned_release(self):
        name = "unsigned-release"
        log(f"attack {name}: anyone with manager access publishes an unsigned release")
        before = self.snapshot()
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.release("unsigned", signed=False)
        if self.protected:
            refusal = wait_until("the Operator to refuse the unsigned release", lambda: next(
                (line for line in self.operator_logs(since).splitlines()
                 if re.search(r"refused the target.*the release is not signed", line)), None), timeout=150)
            # The refusal reaches the manager as a failed update on the next sync.
            wait_until("the manager to show the refusal", lambda: any(
                d["status"] == "update-failed" and "RELEASE_REFUSED" in json.dumps(d.get("error"))
                for d in self.manager_get("/v1/deployments").get("items", [])), timeout=120)
            effect = self.app_message() == "unsigned"
            after = self.snapshot()
            changed = sorted(k for k in set(before) | set(after) if before.get(k) != after.get(k))
            passed = not effect and not changed
        else:
            refusal = None
            try:
                wait_until("the unsigned release", lambda: self.app_message() == "unsigned", timeout=180)
                effect = True
            except TimeoutError:
                effect = False
            changed = []
            passed = effect
        result = {"attack": name, "summary": "an unsigned release", "refusal": refusal,
                  "took_effect": effect, "changed": changed, "passed": passed}
        self.save(f"attacks/{name}/operator.log", self.operator_logs(since))
        self.save(f"attacks/{name}/result.json", result)
        log(f"attack {name}: {'PASS' if passed else 'FAIL'} (took effect: {effect}, changed: {changed})")
        self.results.append(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", choices=["protected", "unprotected"], default="protected")
    parser.add_argument("--evidence", default=None, help="evidence directory (default: a new temporary one)")
    parser.add_argument("--alien", help="alien CLI binary (default: <binaries>/alien)")
    parser.add_argument("--binaries", help="directory with the debug binaries (default: target/debug)")
    parser.add_argument("--only", choices=["protection-turned-on"],
                        help="run one protected-mode step instead of the whole suite")
    parser.add_argument("--down", action="store_true", help="remove everything the suite created")
    args = parser.parse_args()
    args.evidence = args.evidence or tempfile.mkdtemp(prefix="sign-e2e-")
    suite = Suite(args)
    if args.down:
        suite.down()
        return
    suite.up()
    if args.only == "protection-turned-on":
        suite.protected = True
        suite.release("release-1", True)
        suite.install(protected=False)
        suite.wait_for_message("release-1")
        passed = suite.run_protection_turned_on()["passed"]
    else:
        passed = suite.suite()
    print()
    print(f"{'attack':32} {'took effect':12} result")
    for result in suite.results:
        print(f"{result['attack']:32} {str(result['took_effect']):12} {'PASS' if result['passed'] else 'FAIL'}")
    print(f"\nmode {suite.mode}: {'all passed' if passed else 'FAILED'}; evidence in {suite.evidence}")
    sys.exit(0 if passed else 1)


if __name__ == "__main__":
    main()
