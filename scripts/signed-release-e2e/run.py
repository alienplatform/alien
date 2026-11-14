#!/usr/bin/env python3
"""End-to-end attack suite for protected mode (signed releases).

Runs a real `alien-manager`, a real Operator in a kind cluster, and a real
signed `alien release` of a small app. A proxy between the Operator and the
manager plays a fully compromised control plane: it rewrites what the manager
sends, one attack at a time. For each attack the suite checks what reached
the cluster.

    python3 scripts/signed-release-e2e/run.py --mode protected
    python3 scripts/signed-release-e2e/run.py --mode unprotected

Signing is set up the way a team sets it up. A TPM simulator (swtpm) holds the
developer's device key. `alien devices init` in a terminal turns signing on,
and `alien devices add-ci` (with a stand-in `gh`) makes the CI key. Every
later release is signed by the CI key through the real CLI.

`protected` installs the Operator from the signed chart the release carries
(the chart pins the Operator build and the signing fingerprint) and expects
every attack to be refused with nothing in the cluster changed. `unprotected`
publishes the same signed releases but installs the ordinary chart, and
expects every attack to take effect: that is what shows the attacks are real.
Either way, a normal signed release and a rollback by re-release must still
roll out.

In protected mode the suite ends with a second deployment: it installs the
ordinary chart, lets the manager start a dynamic container, and then replaces
the installation with the signed chart. The container must be removed.
`--only protection-turned-on` runs just that step. (It runs last because the
manager decides whether a deployment is protected when it registers.)

Before that, protected mode runs one case for deployer inputs: an optional input
left unset, an input with a signed default, and a deployer secret the installer
puts in the cluster. The workload must start with the default, without the unset
variable, and with the secret's value. `--only deployer-inputs` runs just that
case.

The CLI refuses to sign a developer's input against a standalone manager, so the
suite signs `config/v1` statements itself with the CI key and keeps the CLI's
refusal as evidence.

Needs docker, kind, kubectl, helm, swtpm, tpm2-tools and script on PATH, and
debug builds of `alien-manager` and `alien-operator` (`cargo build -p
alien-manager -p alien-operator`) in target/debug or --binaries. The CLI has
to carry the Operator pin of the image the suite publishes, so the suite
builds it with `cargo build -p alien-cli --features test-device-env`, with the
pin written into two tracked files for the build and restored afterwards (or
pass a CLI built that way with --alien). Evidence (logs, snapshots, the
summary) goes to --evidence. Everything it creates is named `sign-e2e*`;
`--down` removes it.
"""

import argparse
import atexit
import base64
import copy
import hashlib
import json
import os
import re
import secrets
import shlex
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec

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
PROJECT = "default"
REPLAY_ATTACKS = ("older-signed-config", "config-without-base", "config-made-from-an-older-config",
                  "config-made-from-a-refused-config")
# The PIN the swtpm-backed device key is created with (read from the
# environment by test builds of the CLI).
DEVICE_PIN = "4321"
CI_REPOSITORY = "acme/app"


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


def public_key_text(private_key):
    """A signer list key: `ecdsa-p256:` and the base64 DER public key."""
    return "ecdsa-p256:" + base64.b64encode(private_key.public_key().public_bytes(
        serialization.Encoding.DER, serialization.PublicFormat.SubjectPublicKeyInfo)).decode()


def sign_statement(document, private_key, signer):
    """A signed statement: the payload bytes, base64, and `signer`'s signature over them."""
    payload = json.dumps(document, separators=(",", ":")).encode()
    signature = private_key.sign(payload, ec.ECDSA(hashes.SHA256()))
    return {"payload": base64.b64encode(payload).decode(),
            "signatures": [{"signer": signer, "sig": base64.b64encode(signature).decode()}]}


class Suite:
    def __init__(self, args):
        self.mode = args.mode
        self.protected = args.mode == "protected"
        self.evidence = Path(args.evidence).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = self.evidence / "work"
        if not args.down:
            if self.work.exists():
                shutil.rmtree(self.work)
            self.work.mkdir()
            self.attack_dir = self.work / "attack"
            self.attack_dir.mkdir()
            self.app = self.work / "app"
            shutil.copytree(HERE / "app", self.app)
        self.binaries = Path(args.binaries or REPO / "target/debug").resolve()
        self.alien = Path(args.alien).resolve() if args.alien else None
        self.admin_token = "ax_admin_" + secrets.token_hex(16)
        # Sync changes the proxy makes all the time (see `relay_config`).
        self.standing = []
        self.results = []
        self.releases = []
        self.targets = "linux-x64"
        self.swtpm = None
        self.install_name = "customer"
        self.ci_key = None
        # The Operator image the manager's charts run, through the manager.
        self.manager_operator_image = UPSTREAM_OPERATOR

    # ── Environment ──────────────────────────────────────────────────────

    def cli_env(self, who="alice"):
        """The environment of `alien` run as `who`: `alice` is the developer's
        machine (its device key lives in the TPM simulator), `ci` signs with
        the CI key, `stranger` is a machine nobody trusts."""
        home = self.work / "people" / who
        home.mkdir(parents=True, exist_ok=True)
        (home / "gitconfig").write_text(f"[user]\n\temail = {who}@example.com\n")
        env = dict(os.environ)
        env.update(
            {
                "ALIEN_MANAGER_URL": f"http://localhost:{MANAGER_PORT}",
                "ALIEN_API_KEY": self.admin_token,
                "XDG_CONFIG_HOME": str(self.work / "config"),
                "ALIEN_SIGNING_HOME": str(home / "signing"),
                "ALIEN_DEVICE_KEY_PIN": DEVICE_PIN,
                "TPM2TOOLS_TCTI": self.tcti,
                "USER": who,
                "GIT_CONFIG_GLOBAL": str(home / "gitconfig"),
                "GIT_CONFIG_NOSYSTEM": "1",
                "PATH": f"{self.work / 'gh'}:{os.environ.get('PATH', '')}",
                "NO_COLOR": "1",
            }
        )
        env.pop("ALIEN_SIGNING_KEY", None)
        if who == "ci":
            env["ALIEN_SIGNING_KEY"] = self.ci_key
        return env

    def alien_cli(self, *args, who="alice", check=True):
        """`alien` without a terminal, as CI runs it."""
        return run([self.alien, *args], env=self.cli_env(who), cwd=self.app, check=check,
                   input="")

    def terminal(self, args, answers=(), who="alice", timeout=900):
        """`alien` in a terminal (`script` gives it one). For each
        `(prompt, answer)`, wait for the prompt, then type the answer. An
        answer can be a function of what the terminal has shown so far.
        Returns the exit status and everything the terminal showed."""
        line = shlex.join([str(self.alien), *map(str, args)])
        child = subprocess.Popen(
            ["script", "-q", "-e", "-c", line, "/dev/null"], cwd=self.app, env=self.cli_env(who),
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        seen = bytearray()

        def read(stream):
            while chunk := stream.read1(4096):
                seen.extend(chunk)

        readers = [threading.Thread(target=read, args=(stream,), daemon=True)
                   for stream in (child.stdout, child.stderr)]
        for reader in readers:
            reader.start()
        position = 0
        for prompt, answer in answers:
            deadline = time.time() + timeout
            while True:
                at = seen.decode(errors="replace").find(prompt, position)
                if at >= 0:
                    position = at + len(prompt)
                    break
                if time.time() > deadline or child.poll() is not None:
                    child.kill()
                    raise RuntimeError(f"no prompt {prompt!r} in:\n{seen.decode(errors='replace')}")
                time.sleep(0.05)
            # The CLI prints a hidden prompt just before turning echo off, which
            # drops what was typed ahead.
            time.sleep(0.5)
            if callable(answer):
                answer = answer(seen.decode(errors="replace").replace("\r", ""))
            child.stdin.write(answer.encode())
            child.stdin.flush()
        try:
            status = child.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            child.kill()
            raise
        for reader in readers:
            reader.join(timeout=5)
        return status, seen.decode(errors="replace").replace("\r", "")

    def start_tpm(self):
        """A TPM simulator holds the developer's device key."""
        self.tcti = os.environ.get("TPM2TOOLS_TCTI")
        if self.tcti:
            return
        state = self.work / "tpm"
        state.mkdir()
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", 0))
            port = probe.getsockname()[1]
        # The swtpm TCTI reaches the control channel on the next port.
        self.swtpm = subprocess.Popen(
            ["swtpm", "socket", "--tpm2", "--tpmstate", f"dir={state}",
             "--server", f"type=tcp,port={port}", "--ctrl", f"type=tcp,port={port + 1}",
             "--flags", "not-need-init,startup-clear", "--pid", f"file={self.work / 'swtpm.pid'}"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        atexit.register(self.swtpm.kill)
        self.tcti = f"swtpm:port={port}"
        wait_until("swtpm", lambda: run(
            ["tpm2_getrandom", "4", "--hex"], check=False,
            env={**os.environ, "TPM2TOOLS_TCTI": self.tcti}).returncode == 0, timeout=20, interval=0.2)

    def fake_gh(self):
        """A `gh` that stores the secret it is given in a file instead of calling GitHub, and
        answers as a correctly set up `release` environment does."""
        bin_dir = self.work / "gh"
        bin_dir.mkdir()
        secret = self.work / "secret.pem"
        script = f"""#!/bin/sh
case "$*" in
  "secret set"*) cat > '{secret}' ;;
  *"/rulesets/1") echo '{{"id":1,"name":"Release tags","target":"tag","enforcement":"active","conditions":{{"ref_name":{{"include":["refs/tags/v*"],"exclude":[]}}}},"rules":[{{"type":"creation"}},{{"type":"update"}},{{"type":"deletion"}}],"bypass_actors":[]}}' ;;
  *"/rulesets") echo '[{{"id":1,"target":"tag"}}]' ;;
  *"deployment-branch-policies"*"-X POST"*|*"-X POST"*"deployment-branch-policies"*) echo '{{}}' ;;
  *"deployment-branch-policies"*) echo '{{"branch_policies":[{{"name":"main","type":"branch"}},{{"name":"v*","type":"tag"}}]}}' ;;
  *"-X PUT"*) cat > /dev/null; echo '{{}}' ;;
  *"environments/release"*) echo '{{"deployment_branch_policy":{{"protected_branches":false,"custom_branch_policies":true}}}}' ;;
  *) echo "unexpected gh $*" >&2; exit 1 ;;
esac
"""
        gh = bin_dir / "gh"
        gh.write_text(script)
        gh.chmod(0o755)
        return secret

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
        self.operator_digest = self.registry_digest("upstream/alien-operator", "dev")
        log(f"the vendor's Operator build is {self.operator_digest}")
        if not self.alien:
            self.build_cli()

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
        self.turn_signing_on()

    def registry_digest(self, repository, tag):
        """The digest the plain registry gives a pushed image."""
        headers = run(["curl", "-sI", "-H", "Accept: application/vnd.oci.image.manifest.v1+json,"
                       "application/vnd.docker.distribution.manifest.v2+json",
                       f"http://localhost:{REGISTRY_PORT}/v2/{repository}/manifests/{tag}"]).stdout
        return re.search(r"(?im)^docker-content-digest: (sha256:[0-9a-f]{64})", headers).group(1)

    def build_cli(self):
        """A CLI that carries the Operator pin of the published image. Source
        builds carry none, so they can't render a signed chart. The pin is in
        two tracked files for the length of the build only."""
        pins = [(REPO / "crates/alien-core/operator-digest.txt", f"{self.operator_digest}\n"),
                (REPO / "crates/alien-cli/enforcer-digests.json",
                 json.dumps({"operator": self.operator_digest}) + "\n")]
        saved = [(path, path.read_bytes()) for path, _ in pins]
        log("building the CLI with the Operator pin")
        try:
            for path, text in pins:
                path.write_text(text)
            run(["cargo", "build", "-p", "alien-cli", "--features", "test-device-env"],
                cwd=REPO, capture=False)
        finally:
            for path, original in saved:
                path.write_bytes(original)
        self.alien = self.work / "alien"
        shutil.copy2(self.binaries / "alien", self.alien)

    @staticmethod
    def recovery_tail(shown):
        """The last four characters of the recovery code the terminal showed."""
        after = shown.split("Recovery code (shown once", 1)[1].split("\n", 1)[1]
        code = re.search(r"\b([0-9A-Z]{4}(?:-[0-9A-Z]{4}){7})\b", after)
        if code is None:
            raise RuntimeError(f"no recovery code in:\n{shown}")
        return code.group(1)[-4:] + "\n"

    def turn_signing_on(self):
        """The developer runs `alien devices init` in a terminal, which turns
        signing on; then CI is added, and signs everything after."""
        self.start_tpm()
        secret = self.fake_gh()
        log("the developer turns signing on with `alien devices init`")
        status, output = self.terminal(["devices", "init"], [("characters of the code to confirm", self.recovery_tail)])
        self.save("signing/init.txt", output)
        if status != 0:
            raise RuntimeError(f"devices init failed:\n{output}")
        pin = json.loads((self.app / ".alien/signing.json").read_text())
        self.fingerprint = pin["fingerprint"]
        log(f"signing fingerprint {self.fingerprint}")
        status, output = self.terminal(["devices", "add-ci", CI_REPOSITORY])
        self.save("signing/add-ci.txt", output)
        if status != 0:
            raise RuntimeError(f"devices add-ci failed:\n{output}")
        self.ci_key = secret.read_text()
        self.save("signing/devices.txt", self.alien_cli("devices", "list").stdout)

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
        pid_file = self.work / "swtpm.pid"
        if pid_file.exists():
            run(["kill", pid_file.read_text().strip()], check=False)
        for name in (MANAGER, PROXY, REGISTRY):
            run(["docker", "rm", "-f", name], check=False)
        run(["kind", "delete", "cluster", "--name", CLUSTER], check=False)


    # ── Signed config ────────────────────────────────────────────────────

    def ci_private_key(self):
        return serialization.load_pem_private_key(self.ci_key.encode(), None)

    def signer_list_version(self):
        """The newest signer list version the manager holds."""
        lists = self.manager_get(f"/v1/projects/{PROJECT}/signer-lists")["items"]
        return max(json.loads(base64.b64decode(item["payload"]))["sequence"] for item in lists)

    def install_id(self, cluster_uid=None):
        """What a protected Operator calls itself: the cluster, namespace and Helm release."""
        uid = cluster_uid or kubectl("get", "namespace", "kube-system", "-o",
                                     "jsonpath={.metadata.uid}").stdout.strip()
        return f"k8s:{uid}:{NAMESPACE}/{RELEASE_NAME}"

    def ci_signer(self):
        """The id `devices add-ci` gave the CI key, from the newest signer list."""
        lists = self.manager_get(f"/v1/projects/{PROJECT}/signer-lists")["items"]
        newest = max((json.loads(base64.b64decode(item["payload"])) for item in lists),
                     key=lambda statement: statement["sequence"])
        return next(s["id"] for s in newest["body"]["signers"] if s["kind"] == "ci")

    def signed_config(self, body, key=None, signer=None, install=None, base=None):
        """A `config/v1` statement for this installation, signed by CI unless told otherwise.
        `base` is the config it was made from, as `alien deployments set-inputs` names it."""
        signer = signer or self.ci_signer()
        if base is not None:
            payload = base64.b64decode(base["payload"])
            body = {**body, "base": {"sequence": json.loads(payload)["sequence"],
                                     "digest": "sha256:" + hashlib.sha256(payload).hexdigest()}}
        return sign_statement({
            "type": "config/v1", "project": PROJECT, "install": install or self.install_id(),
            "sequence": int(time.time() * 1000), "notAfter": None,
            "signerListVersion": self.signer_list_version(), "body": body,
        }, key or self.ci_private_key(), signer)

    # ── Vendor and installer actions ─────────────────────────────────────

    def release_ids(self):
        return [r["id"] for r in self.manager_get("/v1/releases").get("items", [])]

    def release(self, message, who="ci"):
        """`alien release` of the app printing `message`, as `who`: `ci` signs
        with the CI key, `alice` with her TPM-held device key (in a terminal),
        and `stranger` is a machine nobody trusts, so the release is unsigned."""
        # The manager rolls a release out only to deployments in these statuses
        # (an Operator restarting after a self-update isn't one, for a moment).
        if self.releases and self.manager_get("/v1/deployments").get("items"):
            kubectl("-n", NAMESPACE, "rollout", "status", f"deployment/{RELEASE_NAME}", "--timeout=180s")
            wait_until("the deployment to take a new release", lambda: all(
                d["status"] in ("running", "update-failed", "refresh-failed")
                for d in self.manager_get("/v1/deployments").get("items", [])), timeout=180)
            time.sleep(35)  # one more sync, so the manager's status is current
        config_path = self.app / "alien.json"
        config = json.loads(config_path.read_text())
        config["resources"]["app"]["config"]["code"]["toolchain"]["buildArgs"] = {"MESSAGE": message}
        config_path.write_text(json.dumps(config, indent=2) + "\n")
        before = self.release_ids()
        args = ["release", "--platforms", "kubernetes", "--targets", self.targets, "--no-git"]
        if who == "alice":
            status, output = self.terminal(args)
            if status != 0:
                raise RuntimeError(f"alien release failed:\n{output}")
        else:
            output = self.alien_cli(*args, who=who).stdout
        new = [i for i in self.release_ids() if i not in before]
        if len(new) != 1:
            raise RuntimeError(f"expected one new release, found {new}:\n{output}")
        release = self.manager_get(f"/v1/releases/{new[0]}")
        image = release["stack"]["kubernetes"]["resources"]["app"]["config"]["code"]["image"]
        signed = bool(release.get("signedStacks", {}).get("kubernetes"))
        self.releases.append({"message": message, "signed": signed, "id": new[0], "image": image})
        self.save(f"releases/{len(self.releases):02d}-{message}.txt", output)
        log(f"released {message} ({'signed' if signed else 'unsigned'}, as {who}) as {new[0]}: {image}")
        return release

    def deployments(self, name=None):
        """The manager's deployments, those named `name` if given."""
        items = self.manager_get("/v1/deployments").get("items", [])
        return [d for d in items if name is None or d["name"] == name]

    def deployment_id(self, name="customer"):
        """The deployment `name`, once its installation registered it."""
        return self.deployments(name)[0]["id"]

    def manager_post(self, path, body):
        result = run(["curl", "-sf", "-X", "POST", "-H", f"Authorization: Bearer {self.admin_token}",
                      "-H", "content-type: application/json", "--data-binary", "@-",
                      f"http://localhost:{MANAGER_PORT}{path}"], input=json.dumps(body))
        return result.stdout

    def manager_get(self, path):
        result = run(["curl", "-sf", "-H", f"Authorization: Bearer {self.admin_token}",
                      f"http://localhost:{MANAGER_PORT}{path}"])
        return json.loads(result.stdout)

    # ── Installing ───────────────────────────────────────────────────────

    def onboard(self, name):
        """A deployment link for the customer `name`; returns its token."""
        onboard = json.loads(self.alien_cli("onboard", name, "--platforms", "kubernetes", "--json").stdout)
        self.save(f"onboard-{name}.json", {**onboard, "token": "<redacted>"})
        return onboard

    def install(self, protected=None, name="customer"):
        """Install the Operator in a fresh namespace: the signed chart the
        release carries when protected, the ordinary chart otherwise. The
        deployment registers with the manager as `name`."""
        protected = self.protected if protected is None else protected
        kubectl("delete", "namespace", NAMESPACE, "--ignore-not-found", "--wait=true", "--timeout=180s")
        onboard = self.onboard(name)
        self.install_name = name
        self.management_token = onboard["token"]
        self.chart = onboard["helm"]["chart"].replace(PUBLIC_URL.removeprefix("http://"), f"localhost:{MANAGER_PORT}")
        run(["helm", "registry", "login", f"localhost:{MANAGER_PORT}", "--insecure",
             "--username", "customer", "--password-stdin"], input=onboard["token"])
        self.helm("install", protected)

    def signed_chart(self):
        """The signed chart the newest signed release carries, written to a directory."""
        release = self.manager_get(f"/v1/releases/{next(r for r in reversed(self.releases) if r['signed'])['id']}")
        files = json.loads(base64.b64decode(release["setupTemplates"]["kubernetes"]["helm"]))
        directory = self.work / "signed-chart"
        shutil.rmtree(directory, ignore_errors=True)
        for path, contents in files.items():
            destination = directory / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(contents)
        return directory

    def helm(self, command, protected):
        """`helm install` or `helm upgrade` with the customer's values."""
        values = {
            "management": {"token": self.management_token, "name": self.install_name,
                           "defaultUrl": PUBLIC_URL},
            "inputValues": {"databaseUrl": CUSTOMER_DATABASE_URL},
            "tunnel": {"enabled": False},
        }
        chart = self.chart
        if protected:
            chart = self.signed_chart()
            # The vendor's Operator build, mirrored: the chart fixes its digest.
            values["runtime"] = {"image": {"repository": f"{REGISTRY_HOST}/upstream/alien-operator"}}
        values_file = self.work / "values.yaml"
        values_file.write_text(json.dumps(values))
        (self.evidence / "customer-values.json").write_text(json.dumps(
            {**values, "management": {**values["management"], "token": "<redacted>"}}, indent=2))
        run(["helm", command, RELEASE_NAME, chart, "--plain-http", "--kube-context", CONTEXT,
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

    def app_env(self):
        """The environment the app's workload is given."""
        workload = self.app_workload()
        return {e["name"]: e.get("value") for c in workload["spec"]["template"]["spec"]["containers"]
                for e in c.get("env", [])}

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
            for d in self.deployments(self.install_name)), timeout=timeout)

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
        attack = {**attack, "sync": self.standing + attack.get("sync", [])}
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

        app_env = self.app_env

        def tampered_payload():
            modified = copy.deepcopy(target)
            payload = json.loads(base64.b64decode(modified["releaseInfo"]["signed"]["payload"]))
            # A newer sequence too, so the release is not simply the one already applied.
            payload["sequence"] += 1000
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
            """A signer list version n+1 that adds the attacker's key, claiming an
            admin's id, and a release the attacker signs with it: what a control
            plane that forged the list would send."""
            attacker = ec.generate_private_key(ec.SECP256R1())
            modified = copy.deepcopy(target)
            lists = modified["releaseInfo"]["signerLists"]
            newest = json.loads(base64.b64decode(lists[-1]["payload"]))
            newest_digest = "sha256:" + hashlib.sha256(base64.b64decode(lists[-1]["payload"])).hexdigest()
            body = copy.deepcopy(newest["body"])
            body["previous"] = newest_digest
            body["signers"].append({"id": "attacker", "key": public_key_text(attacker), "kind": "ci",
                                    "roles": ["release", "admin"]})
            forged = {**newest, "sequence": newest["sequence"] + 1,
                      "signerListVersion": newest["sequence"], "body": body}
            admin = next(s["id"] for s in newest["body"]["signers"]
                         if "admin" in s["roles"] and s["kind"] != "recovery")
            modified["releaseInfo"]["signerLists"] = [*lists, sign_statement(forged, attacker, admin)]
            release = json.loads(base64.b64decode(modified["releaseInfo"]["signed"]["payload"]))
            release["sequence"] += 1000
            release["signerListVersion"] = forged["sequence"]
            release["body"]["stack"]["resources"]["app"]["config"]["command"] = evil_command
            release["body"]["workloads"][0]["command"] = evil_command
            modified["releaseInfo"]["signed"] = sign_statement(release, attacker, "attacker")
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
                "summary": "the manager forges the next signer list with its own key and signs a release with it",
                "sync": attacker_signed(),
                "refusal": r"refused the target.*SIGNING_(SIGNATURE|SIGNER_LIST)_REFUSED",
                "took_effect": lambda: self.app_message() == "evil",
            },
            {
                "name": "unsigned-target",
                "summary": "the manager drops the signature and sends its own command",
                "sync": unsigned(),
                "refusal": r"refused the target.*the release is not signed",
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
                "summary": "the manager edits the signed statement itself (new bytes, the old signature)",
                "sync": tampered_payload(),
                "refusal": r"refused the target.*SIGNING_SIGNATURE_REFUSED",
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
        self.release(message)
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
        self.release("release-1")
        self.install()
        self.wait_for_message("release-1")
        self.first_target = wait_until("the first target", lambda: self.target_for("release-1"))
        self.save("normal-deploy/target.json", self.first_target)
        self.save("normal-deploy/snapshot.json", self.snapshot())
        self.save("normal-deploy/app-workload.json", self.app_workload())

        log("normal release: release-2 rolls out with no installer action")
        self.release("release-2")
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
        self.release("release-1")
        self.wait_for_message("release-1")
        self.results.append({"attack": "rollback-by-re-release",
                             "summary": "re-releasing release-1's code rolls it back",
                             "passed": True, "took_effect": True, "changed": []})
        if self.protected:
            self.run_deployer_inputs()
        self.save("operator-full.log", self.operator_logs("2000-01-01T00:00:00Z"))
        if self.protected:
            # A second customer turns protected mode on for an installation they
            # already run. Its own deployment: the manager records whether a
            # deployment is protected when it registers.
            log("protection turned on for an existing installation")
            self.standing = []
            self.install(protected=False, name="early")
            self.wait_for_message(self.current_message())
            self.run_protection_turned_on()
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

    def log_level(self):
        return self.app_env().get("LOG_LEVEL")

    def rolled_out(self):
        """One app pod, none on the way out."""
        selector = ",".join(f"{k}={v}" for k, v in self.app_workload()["spec"]["selector"]["matchLabels"].items())
        pods = json.loads(kubectl("-n", NAMESPACE, "get", "pods", "-l", selector, "-o", "json").stdout)["items"]
        return len(pods) == 1 and not pods[0]["metadata"].get("deletionTimestamp")

    def settled(self):
        """Every deployment runs its release with nothing pending."""
        return all(d["status"] == "running" and not d.get("desiredReleaseId")
                   for d in self.deployments())

    def run_signed_developer_input(self):
        """The developer signs two config/v1 statements that set logLevel; each
        reaches the workload. Then the control plane tries three configs it
        shouldn't deliver."""
        name = "signed-developer-input"
        # Sign on a settled deployment: a config stored while an update is
        # in flight waits for the next change (a known gap in the manager).
        wait_until("the deployment to settle", self.settled, timeout=300)
        deployment = self.deployment_id()
        # `alien deployments set-inputs` is the way to sign it; it refuses
        # a standalone manager, so here it only explains that.
        attempt = self.alien_cli("deployments", "set-inputs", "customer", "--input", "logLevel=warn", check=False)
        self.save(f"attacks/{name}/cli-attempt.txt",
                  f"exit {attempt.returncode}\n{attempt.stdout}{attempt.stderr}")
        log(f"{name}: the developer signs logLevel=warn for this deployment")
        first = self.signed_config({"inputs": {"logLevel": "warn"}})
        self.manager_post(f"/v1/deployments/{deployment}/config-statements", {"statement": first})
        try:
            wait_until("LOG_LEVEL=warn", lambda: self.log_level() == "warn", timeout=240)
            effect = True
        except TimeoutError:
            effect = False
        result = {"attack": name, "took_effect": effect, "changed": [], "passed": effect,
                  "summary": "a signed developer input reaches the workload"}
        self.save(f"attacks/{name}/result.json", result)
        log(f"{name}: {'PASS' if effect else 'FAIL'} (took effect: {effect})")
        self.results.append(result)

        name = "signed-developer-input-update"
        log(f"{name}: the developer signs logLevel=error")
        second = self.signed_config({"inputs": {"logLevel": "error"}}, base=first)
        self.manager_post(f"/v1/deployments/{deployment}/config-statements", {"statement": second})
        try:
            wait_until("LOG_LEVEL=error", lambda: self.log_level() == "error", timeout=240)
            updated = True
        except TimeoutError:
            updated = False
        result = {"attack": name, "took_effect": updated, "changed": [], "passed": updated,
                  "summary": "a newer signed config replaces the one before it"}
        self.save(f"attacks/{name}/result.json", result)
        log(f"{name}: {'PASS' if updated else 'FAIL'}")
        self.results.append(result)
        applied = effect and updated
        if not applied:
            # The manager didn't deliver them. Does the Operator apply them
            # when they reach it? Hand them over the way the manager should.
            applied = self.run_relayed_config(first, second)
        self.run_config_attacks(first, replay=applied)

    @staticmethod
    def relay_config(statement):
        """What a manager that delivers signed config adds to every sync, with
        the newest `config/v1` the developer signed."""
        return [{"op": "set", "path": ["signedConfig"], "value": statement},
                {"op": "set", "path": ["target", "signedConfig"], "value": statement}]

    def run_relayed_config(self, first, second):
        """Signed configs the manager didn't deliver, handed to the Operator through
        the proxy, as a manager that delivered them would. Once the Operator has
        applied one, the proxy keeps adding the newest to every sync: a protected
        Operator that accepted a config refuses any release delivered without
        one, so later releases need it."""
        name = "signed-config-relayed"
        log(f"{name}: the proxy hands the Operator the two signed configs")
        applied = []
        try:
            for statement, value in ((first, "warn"), (second, "error")):
                self.load_attack({"name": name, "sync": self.relay_config(statement)[:1]})
                try:
                    wait_until(f"LOG_LEVEL={value}", lambda: self.log_level() == value, timeout=240)
                    applied.append(True)
                except TimeoutError:
                    applied.append(False)
                    break
            if applied[-1:] == [True]:
                self.standing = self.relay_config(statement)
        finally:
            self.clear_attack()
        passed = applied == [True, True]
        result = {"attack": name, "took_effect": passed, "changed": [], "passed": passed,
                  "summary": "the Operator applies signed configs, newest last, when they reach it"}
        self.save(f"attacks/{name}/result.json", result)
        log(f"{name}: {'PASS' if passed else 'FAIL'}")
        self.results.append(result)
        return passed

    def run_config_attacks(self, older, replay):
        """The control plane hands the Operator a config it must not apply. The
        newest applied one sets logLevel=error; none of these may change it.
        The replay of the older config means something only if the newer one
        was applied (`replay`)."""
        wait_until("the app to finish rolling out", self.rolled_out, timeout=180)
        attacker = ec.generate_private_key(ec.SECP256R1())
        target = self.baseline_target
        configs = [
            {
                "name": "forged-config",
                "summary": "the manager signs logLevel=debug with its own key, under the CI key's id",
                "sync": [{"op": "set", "path": ["signedConfig"], "value": self.signed_config(
                    {"inputs": {"logLevel": "debug"}}, key=attacker)}],
                "refusal": r"refused the target.*SIGNING_SIGNATURE_REFUSED",
            },
            {
                "name": "config-for-another-install",
                "summary": "the manager sends a config the developer signed for a different cluster",
                "sync": [{"op": "set", "path": ["signedConfig"], "value": self.signed_config(
                    {"inputs": {"logLevel": "debug"}},
                    install=self.install_id(cluster_uid="00000000-0000-4000-8000-000000000000"))}],
                "refusal": r"refused the target.*SIGNING_WRONG_INSTALL",
            },
            {
                "name": "older-signed-config",
                "summary": "the manager replays the developer's earlier config (logLevel=warn)",
                "sync": [{"op": "delete", "path": ["signedConfig"]},
                         {"op": "set", "path": ["target"], "value": {**target, "signedConfig": older}}],
                "refusal": r"refused the target.*(the config has sequence.*already applied"
                           r"|doesn't say which config it was made from)",
            },
            {
                "name": "config-without-base",
                "summary": "the manager sends a newer config the developer signed without naming the config it was made from",
                "sync": [{"op": "set", "path": ["signedConfig"], "value": self.signed_config(
                    {"inputs": {"logLevel": "debug"}})}],
                "refusal": r"refused the target.*doesn't say which config it was made from",
            },
            {
                "name": "config-made-from-an-older-config",
                "summary": "the manager sends a newer config made from the first config, which skips the one the install runs",
                "sync": [{"op": "set", "path": ["signedConfig"], "value": self.signed_config(
                    {"inputs": {"logLevel": "debug"}}, base=older)}],
                "refusal": r"refused the target.*signed on top of config sequence.*never accepted",
            },
            {
                "name": "config-made-from-a-refused-config",
                "summary": "the manager sends a config made from a newer config the install refused, whose stale values it copied",
                "sync": [{"op": "set", "path": ["signedConfig"], "value": self.signed_config(
                    {"inputs": {"logLevel": "debug"}},
                    base=self.signed_config({"inputs": {"logLevel": "debug"}}))}],
                "refusal": r"refused the target.*signed on top of config sequence.*never accepted",
            },
        ]
        level = self.log_level()
        for config in configs:
            if config["name"] in REPLAY_ATTACKS and not replay:
                log(f"attack {config['name']}: not run, the newer signed config was never applied")
                self.results.append({"attack": config["name"], "summary": config["summary"],
                                     "took_effect": None, "changed": [], "passed": False,
                                     "note": "not run: no signed config was applied to replay against"})
                continue
            config["took_effect"] = lambda: self.log_level() != level
            self.run_attack(config)
        if self.protected:
            # A refused target stays with its claim and the deployment shows
            # update-failed until a newer target replaces it.
            self.restore()

    def run_registry_swap(self):
        """The registry serves other bytes for the release's image; the node has to pull again."""
        name = "registry-swap"
        log(f"attack {name}: the registry proxy serves an unreleased image for the app's reference")
        self.push_evil_image()
        if not self.protected:
            # Unsigned releases reference images by tag, as they always have.
            self.release("tagged", who="stranger")
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

    def publish_unsigned_directly(self):
        """What a stranger with manager access does: `alien release` itself
        refuses to publish an unsigned release to a channel protected
        deployments follow, so the request goes straight to the manager,
        pointing the stack at the image the vendor never released."""
        if not hasattr(self, "evil_manifest"):
            self.push_evil_image()
        previous = self.manager_get(f"/v1/releases/{self.releases[-1]['id']}")
        stack = copy.deepcopy(previous["stack"])
        code = stack["kubernetes"]["resources"]["app"]["config"]["code"]
        code["image"] = re.sub(r"[:@][^/]*$", ":evil", code["image"])
        before = self.release_ids()
        self.manager_post("/v1/releases", {"projectId": "default", "stack": stack})
        new = [i for i in self.release_ids() if i not in before]
        if len(new) != 1:
            raise RuntimeError(f"expected one new release, found {new}")
        log(f"released an unsigned stack as stranger as {new[0]}: {code['image']}")

    def app_pod_env(self):
        """The variables the running app pod printed at start, or None."""
        workload = self.app_workload()
        selector = ",".join(f"{k}={v}" for k, v in workload["spec"]["selector"]["matchLabels"].items())
        pods = json.loads(kubectl("-n", NAMESPACE, "get", "pods", "-l", selector, "-o", "json").stdout)["items"]
        running = [p for p in pods if p["status"].get("phase") == "Running" and not p["metadata"].get("deletionTimestamp")]
        if len(running) != 1:
            return None
        logs = kubectl("-n", NAMESPACE, "logs", running[0]["metadata"]["name"], check=False).stdout
        if not re.search(r"^started: ", logs, re.M):
            return None
        return dict(line.split("=", 1) for line in logs.splitlines() if re.match(r"^[A-Z_][A-Z0-9_]*=", line))

    def deployer_secret_location(self, input_id):
        """Where the installer writes the secret input `input_id`: the Secret's
        name and namespace, as the Operator reports them to the manager, or None."""
        for deployment in self.deployments(self.install_name):
            full = self.manager_get(f"/v1/deployments/{deployment['id']}")
            metadata = full.get("runtimeMetadata") or full.get("runtime_metadata") or {}
            for report in metadata.get("deployerSecrets", []):
                if report["inputId"] == input_id:
                    location = report["location"]
                    namespace = re.search(r"--namespace (\S+)", location["cliCommand"])
                    return {"name": location["name"], "namespace": namespace and namespace.group(1).strip("'")}
        return None

    def run_deployer_inputs(self):
        """What a protected workload gets from the installer's inputs, resolved
        from the signed release alone: an optional input nobody set is left out
        of the workload, an input with a signed default takes the default, and
        a deployer secret reaches the container with its value."""
        name = "deployer-inputs"
        log(f"{name}: optional input unset, input with a signed default, deployer secret")
        wait_until("the deployment to settle", self.settled, timeout=300)
        self.release("release-inputs-1")
        self.wait_for_message("release-inputs-1")
        location = wait_until("the Operator to report where the secret goes",
                              lambda: self.deployer_secret_location("apiToken"), timeout=240)
        if not location["namespace"]:
            raise RuntimeError(f"no namespace in the reported secret location: {location}")
        token = "fake-token-" + secrets.token_hex(8)
        run(["kubectl", "--context", CONTEXT, "-n", location["namespace"], "create", "secret", "generic",
             location["name"], f"--from-literal=value={token}"])
        self.release("release-inputs-2")
        self.wait_for_message("release-inputs-2")
        env = wait_until("the app pod's environment", self.app_pod_env, timeout=120)
        spec = self.app_env()
        checks = {
            "the signed default is used": env.get("MODE") == "fast",
            "the unset optional input is left out of the pod": "REGION" not in env,
            "the unset optional input is left out of the workload": "REGION" not in spec,
            "the deployer secret has its value": env.get("API_TOKEN") == token,
            "the installer's value is used": env.get("DATABASE_URL") == CUSTOMER_DATABASE_URL,
        }
        passed = all(checks.values())
        result = {"attack": name, "took_effect": passed, "changed": [], "passed": passed,
                  "summary": "unset optional input left out, signed default used, deployer secret delivered",
                  "checks": checks, "secret": location}
        self.save(f"attacks/{name}/result.json", result)
        self.save(f"attacks/{name}/pod-env.json", {k: ("<redacted>" if k == "API_TOKEN" else v) for k, v in env.items()})
        log(f"{name}: {'PASS' if passed else 'FAIL'} {checks}")
        self.results.append(result)
        return result

    def run_unsigned_release(self):
        name = "unsigned-release"
        log(f"attack {name}: anyone with manager access publishes an unsigned release")
        before = self.snapshot()
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        if self.protected:
            self.publish_unsigned_directly()
        else:
            self.release("unsigned", who="stranger")
        if self.protected:
            refusal = wait_until("the Operator to refuse the unsigned release", lambda: next(
                (line for line in self.operator_logs(since).splitlines()
                 if re.search(r"refused the target.*the release is not signed", line)), None), timeout=150)
            # The refusal reaches the manager as a failed update on the next sync.
            wait_until("the manager to show the refusal", lambda: any(
                d["status"] == "update-failed" and "RELEASE_REFUSED" in json.dumps(d.get("error"))
                for d in self.manager_get("/v1/deployments").get("items", [])), timeout=120)
            effect = self.app_message() == "evil"
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
    parser.add_argument("--alien", help="alien CLI built with the Operator pin and the test-device-env "
                                        "feature (default: build it)")
    parser.add_argument("--binaries", help="directory with the debug binaries (default: target/debug)")
    parser.add_argument("--only", choices=["protection-turned-on", "deployer-inputs", "setup"],
                        help="run one step instead of the whole suite")
    parser.add_argument("--down", action="store_true", help="remove everything the suite created")
    args = parser.parse_args()
    args.evidence = args.evidence or tempfile.mkdtemp(prefix="sign-e2e-")
    suite = Suite(args)
    if args.down:
        suite.down()
        return
    suite.up()
    if args.only == "setup":
        suite.release("release-1")
        log(f"signed chart in {suite.signed_chart()}")
        return
    if args.only == "deployer-inputs":
        suite.protected = True
        suite.release("release-1")
        suite.install()
        suite.wait_for_message("release-1")
        passed = suite.run_deployer_inputs()["passed"]
    elif args.only == "protection-turned-on":
        suite.protected = True
        suite.release("release-1")
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
