#!/usr/bin/env python3
"""Protected mode of the real Operator in a kind cluster, against a fully
compromised control plane.

A fake manager (fake_manager.py) answers the Operator's syncs with whatever
the test picks: releases and configs signed with fresh keys by the
`protected-kind-fixtures` example, and attacks on them. The test creates
what a protected chart creates (the `<release>-signing` and
`<release>-signing-ledger` ConfigMaps, an Operator Role that can't write
ConfigMaps other than the ledger, and a ClusterRole to read the kube-system
namespace for the install id), runs the Operator image built from the debug
binary, and checks what reaches the cluster.

    scripts/signed-release-e2e/operator_kind.py --mode protected --evidence /tmp/op-kind
    scripts/signed-release-e2e/operator_kind.py --mode unprotected --evidence /tmp/op-kind-off

`unprotected` installs no signing ConfigMaps and expects every attack to take
effect; `protected` expects each refused with its reason and the app
unchanged. Protected also removes the CI key, delivers config, wipes the
Operator's database, and deletes the settings. Names come from
$TEST_RESOURCE_PREFIX; `--down` removes the cluster and the manager.

Needs docker (DOCKER_HOST), kind and kubectl, and debug builds of
`alien-operator` and the `protected-kind-fixtures` example.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMESPACE = "customer"
RELEASE = "signed-e2e"
DEPLOYMENT_ID = "dep_kind"
DATABASE_URL = "postgres://db.customer.internal/app"
IMAGES = {
    "a": "docker.io/library/busybox@sha256:a7dbc319b1438115a207e11930dc4bb3e643dcb3ec0bdd694bb36cd8fad289ec",
    "b": "docker.io/library/busybox@sha256:73aaf090f3d85aa34ee199857f03fa3a95c8ede2ffd4cc2cdb5b94e566b11662",
    "evil": "docker.io/library/alpine@sha256:d9e853e87e55526f6b2917df91a2115c36dd7c696a35be12163d44e6e2a4b6bc",
}


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


def run(args, check=True, input=None):
    result = subprocess.run([str(a) for a in args], capture_output=True, text=True, input=input)
    if check and result.returncode != 0:
        raise RuntimeError(f"{' '.join(map(str, args[:3]))} failed: {result.stderr.strip()[:2000]}")
    return result


def wait_until(what, predicate, timeout=180, interval=3):
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(interval)
    raise TimeoutError(f"timed out waiting for {what}")


class Suite:
    def __init__(self, args):
        prefix = os.environ.get("TEST_RESOURCE_PREFIX") or args.prefix
        if not prefix:
            raise SystemExit("set TEST_RESOURCE_PREFIX")
        self.protected = args.mode == "protected"
        self.cluster = f"{prefix}-op"
        self.context = f"kind-{self.cluster}"
        self.manager = f"{prefix}-op-manager"
        self.image = f"{prefix}-op/alien-operator:dev"
        self.evidence = Path(args.evidence).resolve()
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.sync_dir = self.evidence / "sync"
        self.binaries = Path(args.binaries).resolve()
        self.results = []
        self.accounts = set()

    def kubectl(self, *args, check=True, input=None):
        return run(["kubectl", "--context", self.context, *args], check=check, input=input)

    def apply(self, manifest):
        self.kubectl("apply", "-f", "-", input=json.dumps(manifest))

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    # ── Setup ────────────────────────────────────────────────────────────

    def up(self):
        if self.cluster not in run(["kind", "get", "clusters"]).stdout.split():
            log(f"creating kind cluster {self.cluster}")
            run(["kind", "create", "cluster", "--name", self.cluster, "--config", HERE / "kind.yaml", "--wait", "180s"])
        build = self.evidence / "image"
        shutil.rmtree(build, ignore_errors=True)
        build.mkdir()
        run(["strip", "-o", build / "alien-operator", self.binaries / "alien-operator"])
        (build / "Dockerfile").write_text(
            "FROM debian:bookworm-slim\n"
            "RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates"
            " && rm -rf /var/lib/apt/lists/* && mkdir -p /var/lib/operator && chown 65532:65532 /var/lib/operator\n"
            "COPY alien-operator /app/alien-operator\nENV RUST_MIN_STACK=67108864\nUSER 65532\n"
            'ENTRYPOINT ["/app/alien-operator"]\n')
        run(["docker", "build", "-q", "-t", self.image, build])
        run(["kind", "load", "docker-image", self.image, "--name", self.cluster])
        shutil.rmtree(build)

        uid = json.loads(self.kubectl("get", "namespace", "kube-system", "-o", "json").stdout)["metadata"]["uid"]
        self.install = f"k8s:{uid}:{NAMESPACE}/{RELEASE}"
        fixtures = self.evidence / "fixtures"
        shutil.rmtree(fixtures, ignore_errors=True)
        run([self.binaries / "examples/protected-kind-fixtures", fixtures, RELEASE, self.install,
             IMAGES["a"], IMAGES["b"], IMAGES["evil"]])
        shutil.rmtree(self.sync_dir, ignore_errors=True)
        shutil.copytree(fixtures / "sync", self.sync_dir)
        # The manager names itself as the registry proxy; these images come
        # from elsewhere, so they need no pull secret.
        for path in self.sync_dir.glob("*.json"):
            response = json.loads(path.read_text())
            response["target"]["config"]["managerUrl"] = f"http://{self.manager}:8080"
            path.write_text(json.dumps(response))
        # A newer config alone, as the manager sends it once the target is applied.
        config_only = {"signedConfig": json.loads((self.sync_dir / "config-2.json").read_text())["signedConfig"]}
        (self.sync_dir / "config-2-only.json").write_text(json.dumps(config_only))
        self.settings = (fixtures / "settings.json").read_text()
        self.ledger_seed = (fixtures / "ledger.json").read_text()
        self.serve("release-1")

        run(["docker", "rm", "-f", self.manager], check=False)
        run(["docker", "run", "-d", "--name", self.manager, "--network", "kind",
             "--user", f"{os.getuid()}:{os.getgid()}",
             "-v", f"{HERE / 'fake_manager.py'}:/fake_manager.py:ro", "-v", f"{self.sync_dir}:/sync",
             "-e", f"DEPLOYMENT_ID={DEPLOYMENT_ID}", "python:3.12-alpine", "python3", "-u", "/fake_manager.py"])

        self.kubectl("delete", "namespace", NAMESPACE, "--ignore-not-found", "--wait=true", "--timeout=180s")
        self.apply({"apiVersion": "v1", "kind": "Namespace", "metadata": {"name": NAMESPACE}})
        if self.protected:
            self.write_signing()
        self.apply_operator()

    def write_signing(self):
        keep = {"helm.sh/resource-policy": "keep"}
        for name, key, value in [(f"{RELEASE}-signing", "settings", self.settings),
                                 (f"{RELEASE}-signing-ledger", "ledger", self.ledger_seed)]:
            self.apply({"apiVersion": "v1", "kind": "ConfigMap",
                        "metadata": {"name": name, "namespace": NAMESPACE, "annotations": keep},
                        "data": {key: value}})

    def apply_operator(self):
        sa = "operator"
        core = ["secrets", "services", "pods", "pods/log", "persistentvolumeclaims", "serviceaccounts", "events", "endpoints"]
        rules = [
            {"apiGroups": [""], "resources": core, "verbs": ["*"]},
            {"apiGroups": [""], "resources": ["configmaps"], "verbs": ["get", "list", "watch"]},
            {"apiGroups": [""], "resources": ["configmaps"], "resourceNames": [f"{RELEASE}-signing-ledger"], "verbs": ["update"]},
            {"apiGroups": ["apps"], "resources": ["deployments", "statefulsets", "daemonsets", "replicasets"], "verbs": ["*"]},
            {"apiGroups": ["batch"], "resources": ["jobs", "cronjobs"], "verbs": ["*"]},
            {"apiGroups": ["rbac.authorization.k8s.io"], "resources": ["roles", "rolebindings"], "verbs": ["*"]},
            {"apiGroups": ["networking.k8s.io"], "resources": ["networkpolicies", "ingresses"], "verbs": ["*"]},
            {"apiGroups": ["policy"], "resources": ["poddisruptionbudgets"], "verbs": ["*"]},
            {"apiGroups": ["autoscaling"], "resources": ["horizontalpodautoscalers"], "verbs": ["*"]},
        ]
        manifests = [
            {"apiVersion": "v1", "kind": "ServiceAccount", "metadata": {"name": sa, "namespace": NAMESPACE}},
            {"apiVersion": "rbac.authorization.k8s.io/v1", "kind": "Role", "metadata": {"name": sa, "namespace": NAMESPACE}, "rules": rules},
            {"apiVersion": "rbac.authorization.k8s.io/v1", "kind": "RoleBinding", "metadata": {"name": sa, "namespace": NAMESPACE},
             "subjects": [{"kind": "ServiceAccount", "name": sa, "namespace": NAMESPACE}],
             "roleRef": {"apiGroup": "rbac.authorization.k8s.io", "kind": "Role", "name": sa}},
            {"apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRole", "metadata": {"name": f"{NAMESPACE}-{sa}-namespaces"},
             "rules": [{"apiGroups": [""], "resources": ["namespaces"], "resourceNames": ["kube-system", NAMESPACE], "verbs": ["get"]}]},
            {"apiVersion": "rbac.authorization.k8s.io/v1", "kind": "ClusterRoleBinding", "metadata": {"name": f"{NAMESPACE}-{sa}-namespaces"},
             "subjects": [{"kind": "ServiceAccount", "name": sa, "namespace": NAMESPACE}],
             "roleRef": {"apiGroup": "rbac.authorization.k8s.io", "kind": "ClusterRole", "name": f"{NAMESPACE}-{sa}-namespaces"}},
            {"apiVersion": "v1", "kind": "Secret", "metadata": {"name": "operator-files", "namespace": NAMESPACE},
             "stringData": {"token": "ax_dep_kind", "encryption-key": "0" * 64,
                            "inputs.json": json.dumps({"databaseUrl": DATABASE_URL}),
                            # What the chart writes: the workloads run in the release namespace.
                            "stack-settings.json": json.dumps({"deploymentModel": "pull", "kubernetes": {
                                "cluster": {"namespace": NAMESPACE, "ownership": "external"}}})}},
            {"apiVersion": "apps/v1", "kind": "Deployment", "metadata": {"name": "operator", "namespace": NAMESPACE},
             "spec": {"replicas": 1, "selector": {"matchLabels": {"app": "operator"}},
                      "template": {"metadata": {"labels": {"app": "operator"}}, "spec": {
                          "serviceAccountName": sa,
                          "securityContext": {"runAsUser": 65532, "runAsGroup": 65532, "fsGroup": 65532},
                          "containers": [{"name": "operator", "image": self.image, "imagePullPolicy": "Never",
                                          "env": [{"name": k, "value": v} for k, v in {
                                              "PLATFORM": "kubernetes", "SYNC_URL": f"http://{self.manager}:8080",
                                              "SYNC_TOKEN_FILE": "/files/token", "DEPLOYMENT_ID": DEPLOYMENT_ID,
                                              "OPERATOR_NAME": RELEASE, "OPERATOR_RESOURCE_PREFIX": RELEASE,
                                              "KUBERNETES_NAMESPACE": NAMESPACE, "OPERATOR_SETUP_METHOD": "helm",
                                              "OPERATOR_HELM_RELEASE": RELEASE, "DATA_DIR": "/var/lib/operator",
                                              "OPERATOR_ENCRYPTION_KEY_FILE": "/files/encryption-key",
                                              "STACK_INPUT_VALUES_FILE": "/files/inputs.json",
                                              "STACK_SETTINGS_FILE": "/files/stack-settings.json",
                                              "TUNNEL_ENABLED": "false", "SYNC_INTERVAL": "5", "RUST_LOG": "info",
                                              "OPERATOR_SELF_UPDATE_DEPLOYMENT": "operator",
                                          }.items()],
                                          "volumeMounts": [{"name": "files", "mountPath": "/files"},
                                                           {"name": "data", "mountPath": "/var/lib/operator"}]}],
                          "volumes": [{"name": "files", "secret": {"secretName": "operator-files", "defaultMode": 0o440}},
                                      {"name": "data", "emptyDir": {}}]}}}},
        ]
        for manifest in manifests:
            self.apply(manifest)

    def down(self):
        run(["docker", "rm", "-f", self.manager], check=False)
        run(["kind", "delete", "cluster", "--name", self.cluster], check=False)

    # ── Observing ────────────────────────────────────────────────────────

    def serve(self, name):
        (self.sync_dir / "current.tmp").write_text(name)
        os.replace(self.sync_dir / "current.tmp", self.sync_dir / "current")

    def operator_logs(self, since=None):
        args = ["-n", NAMESPACE, "logs", "deployment/operator", "--tail=-1"]
        if since:
            args.append(f"--since-time={since}")
        return self.kubectl(*args, check=False).stdout

    def app(self):
        """The app container's image and env, from its Deployment. Creates
        the service account a workload runs as, which the chart creates."""
        items = json.loads(self.kubectl("-n", NAMESPACE, "get", "deployments", "-o", "json").stdout)["items"]
        for item in items:
            if item["metadata"]["name"] == "operator":
                continue
            account = item["spec"]["template"]["spec"].get("serviceAccountName")
            if account and account not in self.accounts:
                self.apply({"apiVersion": "v1", "kind": "ServiceAccount", "metadata": {"name": account, "namespace": NAMESPACE}})
                self.accounts.add(account)
            for container in item["spec"]["template"]["spec"]["containers"]:
                env = {e["name"]: e.get("value") for e in container.get("env", [])}
                if "MESSAGE" in env:
                    return {"deployment": item["metadata"]["name"], "image": container["image"], "env": env}
        return None

    def wait_for(self, what, check, timeout=240):
        try:
            return wait_until(what, lambda: (lambda app: app if app and check(app) else None)(self.app()), timeout=timeout)
        except TimeoutError:
            return None

    def ledger(self):
        data = json.loads(self.kubectl("-n", NAMESPACE, "get", "configmap", f"{RELEASE}-signing-ledger", "-o", "json").stdout)
        return json.loads(data["data"]["ledger"])

    # ── Cases ────────────────────────────────────────────────────────────

    def lands(self, name, serve, message, image, env=None, timeout=300):
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.serve(serve)
        app = self.wait_for(f"{message} to run", lambda app: app["env"].get("MESSAGE") == message and app["image"] == IMAGES[image]
                            and all(app["env"].get(k) == v for k, v in (env or {}).items()), timeout=timeout)
        self.record(name, app is not None, json.dumps(app or self.app())[:500])
        if app is not None:
            # A target sent mid-update waits for it, so attacks start from rest.
            self.settled(since)
        return app

    def settled(self, since):
        try:
            wait_until("the Operator to finish its update", lambda: "status=Running stop_reason=Synced" in
                       re.sub(r"\x1b\[[0-9;]*m", "", self.operator_logs(since)), timeout=180)
        except TimeoutError:
            log("the Operator did not report a finished update")

    def attack(self, name, serve, reason, took_effect, settle=40):
        """Protected: refused with `reason` (None: nothing to refuse), and the
        app unchanged. Unprotected: the attack takes effect."""
        before = self.app()
        since = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        self.serve(serve)
        if self.protected:
            refusal = None
            if reason:
                try:
                    refusal = wait_until(f"the Operator to refuse {name}", lambda: next(
                        (line for line in self.operator_logs(since).splitlines() if re.search(reason, line)), None), timeout=120)
                except TimeoutError:
                    refusal = None
            time.sleep(settle)
            after = self.app()
            effect = took_effect(after)
            self.record(f"{name} (on)", (refusal is not None or not reason) and not effect and after == before,
                        f"refusal={(refusal or '')[:400]} effect={effect} unchanged={after == before}")
        else:
            try:
                wait_until(f"{name} to take effect", lambda: took_effect(self.app()), timeout=180)
                effect = True
            except TimeoutError:
                effect = False
            self.record(f"{name} (off)", effect, json.dumps(self.app())[:400])
        (self.evidence / "attacks").mkdir(exist_ok=True)
        (self.evidence / "attacks" / f"{name}.log").write_text(self.operator_logs(since))

    def suite(self):
        message = lambda app: (app or {}).get("env", {}).get("MESSAGE")
        env = lambda app: (app or {}).get("env", {})
        self.lands("release 1 lands", "release-1", "release-1", "a")
        self.lands("release 2 lands", "release-2", "release-2", "b")
        if self.protected:
            self.lands("signed config: the developer input comes from it, the deployer input from the install",
                       "release-2-config-1", "release-2", "b", {"LOG_LEVEL": "debug", "DATABASE_URL": DATABASE_URL})
        else:
            self.serve("release-2-config-1")
        restore = "release-2-config-1"
        attacks = [
            ("unsigned", r"refused the target.*not signed", lambda app: message(app) == "evil"),
            ("modified-stack-image", r"refused the target.*differs from the signed release", lambda app: (app or {}).get("image") == IMAGES["evil"]),
            # Same sequence as the accepted release, other bytes: refused as a replay
            # before the signature is even checked.
            ("tampered-signed-bytes", r"refused the target.*(SIGNATURE|already applied)", lambda app: (app or {}).get("image") == IMAGES["evil"]),
            ("older-signed-release", r"refused the target", lambda app: message(app) == "release-1"),
            ("dev-release", r"refused the target.*channel", lambda app: message(app) == "dev"),
            ("attacker-key", r"refused the target.*[Ss]ignature", lambda app: message(app) == "evil"),
            ("ld-preload-from-vault", r"refused the target", lambda app: "LD_PRELOAD" in str(env(app)) or message(app) == "release-5"),
            ("manager-env-vars", r"refused the target.*environment variables.*LD_PRELOAD", lambda app: "LD_PRELOAD" in env(app) or "NODE_OPTIONS" in env(app)),
            ("redirected-deployer-input", None, lambda app: env(app).get("DATABASE_URL") == "postgres://attacker.example/steal"),
        ]
        for name, reason, took_effect in attacks:
            self.attack(name, name, reason, took_effect)
            if not self.protected:
                # Put the release back before the next attack.
                self.lands(f"restore after {name}", "release-2", "release-2", "b")
            self.serve(restore)
        self.attack("manager-operator-image", "manager-operator-image", r"refused to update the Operator to an image no accepted release signed",
                    lambda app: IMAGES["evil"] in json.dumps(json.loads(self.kubectl("-n", NAMESPACE, "get", "deployment", "operator", "-o", "json").stdout)["spec"]))
        if not self.protected:
            return

        # The CI key leaves the list.
        self.attack("ci-removed: the accepted release keeps running", "ci-removed-accepted-release", None, lambda app: message(app) != "release-2")
        self.lands("ci-removed: the next release, signed by the laptop, lands", "ci-removed-next-release", "release-10", "a", {"LOG_LEVEL": "debug"})
        self.attack("ci-removed: a release CI signs after removal", "ci-removed-ci-release", r"refused the target", lambda app: message(app) == "release-11")
        self.serve("ci-removed-next-release")
        self.lands("newer signed config applies without a release", "config-2-only", "release-10", "a", {"LOG_LEVEL": "warn"})
        ledger = self.ledger()
        self.record("the ledger ConfigMap rose to release 10, config 2, list 2",
                    ledger.get("release", {}).get("sequence") == 10 and ledger.get("config", {}).get("sequence") == 2
                    and ledger.get("signerList", {}).get("sequence") == 2 and ledger.get("install") == self.install, json.dumps(ledger))

        # The Operator loses its database: a new pod, an empty data dir.
        self.kubectl("-n", NAMESPACE, "delete", "pod", "-l", "app=operator", "--wait=true")
        self.kubectl("-n", NAMESPACE, "rollout", "status", "deployment/operator", "--timeout=180s")
        self.attack("database wiped, then an older signed release", "older-signed-release", r"refused the target", lambda app: message(app) == "release-1", settle=30)
        self.serve("config-2")

        # Sticky: the settings disappear.
        self.kubectl("-n", NAMESPACE, "delete", "configmap", f"{RELEASE}-signing")
        self.kubectl("-n", NAMESPACE, "delete", "pod", "-l", "app=operator", "--wait=true")
        try:
            line = wait_until("the Operator to refuse to start", lambda: next(
                (l for l in self.operator_logs().splitlines() if "is gone" in l), None), timeout=120)
        except TimeoutError:
            line = None
        self.record("settings deleted after acceptance: the Operator stops", line is not None and message(self.app()) == "release-10",
                    (line or self.operator_logs()[-600:])[:500])
        self.write_signing_settings_only()
        self.kubectl("-n", NAMESPACE, "delete", "pod", "-l", "app=operator", "--wait=true")
        self.kubectl("-n", NAMESPACE, "rollout", "status", "deployment/operator", "--timeout=180s")
        self.lands("settings restored: the install runs on", "config-2", "release-10", "a", {"LOG_LEVEL": "warn"})

    def write_signing_settings_only(self):
        self.apply({"apiVersion": "v1", "kind": "ConfigMap",
                    "metadata": {"name": f"{RELEASE}-signing", "namespace": NAMESPACE,
                                 "annotations": {"helm.sh/resource-policy": "keep"}},
                    "data": {"settings": self.settings}})


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--mode", choices=["protected", "unprotected"], default="protected")
    parser.add_argument("--evidence", required=True)
    parser.add_argument("--binaries", default=str(Path(os.environ.get("CARGO_TARGET_DIR", HERE.parent.parent / "target")) / "debug"))
    parser.add_argument("--prefix", help="when TEST_RESOURCE_PREFIX is unset")
    parser.add_argument("--down", action="store_true")
    args = parser.parse_args()
    suite = Suite(args)
    if args.down:
        suite.down()
        return
    try:
        suite.up()
        suite.suite()
    except Exception as error:
        suite.record("the run itself", False, f"{type(error).__name__}: {error}")
    finally:
        (suite.evidence / "results.json").write_text(json.dumps(suite.results, indent=2))
        (suite.evidence / "operator.log").write_text(suite.operator_logs())
    failed = [r for r in suite.results if not r["pass"]]
    log(f"{len(suite.results) - len(failed)}/{len(suite.results)} passed")
    sys.exit(1 if failed or not suite.results else 0)


if __name__ == "__main__":
    main()
