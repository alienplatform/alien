#!/usr/bin/env python3
"""A protected install's Container App worker, created and updated by the control plane on real
Azure.

The Azure counterpart of `control_plane_gcp.py`. Sets up, as protected Terraform setup would, an
install in a new resource group `<p>` (every name starts with `<p>`, everything tagged
`--tag`):
  - the signing settings as the group's `signing-fingerprint` tag and the ledger as the
    `signing-state` secret of Key Vault `<p>-kv`;
  - the workload's identity `<p>-execution-sa` and the verifier's `<p>-verifier` (Contributor on
    the group, Key Vault Secrets Officer on the vault);
  - the stack's Container Apps environment `<p>-default-container-env` and, beside it in the same
    group, the verifier's own environment `<p>-verifier`, as setup renders both;
  - a registry `<p>acr` with two worker images and the verifier's, and a pull-only registry token
    named `deployment`, standing in for the manager's registry proxy and the deployment token;
  - the verifier as Container App `<p>-verifier`, with public ingress behind Container Apps
    authentication as setup renders it: Microsoft provider, Resource Manager audience, the caller
    (standing in for management) the only allowed principal.
No setup creates the worker's app. `protected_worker_update update` (alien-infra example,
CLOUD=azure) runs the real stack executor and Azure worker controller with the caller's ARM token
as management, and the script checks:
  0. the first deployment creates `<p>-api` through the verifier: the signed image, in the stack's
     environment, as the workload identity, without ingress (the worker has no public endpoint);
  1. an update moves it to the second image through the verifier;
  2. a release under a signer list setup didn't pin is refused with the verifier's reason, and the
     app keeps the second image;
  3. Azure's activity log names the verifier's identity as the only writer of `<p>-api`;
  4. before the cases: a request with no token gets 401, management's token reaches the verifier
     (its own answer), and with another principal allowed instead, management's token gets 403
     from Container Apps.
Everything is deleted at the end (the vault purged) and the run fails if anything is left.

    scripts/verifier-e2e/control_plane_azure.py \\
        --verifier target/x86_64-unknown-linux-musl/debug/alien-verifier \\
        --driver target/debug/examples/protected_worker_update \\
        --token-file token.json --prefix e2e-cpz --tag purpose=e2e --evidence /tmp/control-plane-azure

The token file holds `accessToken` (ARM scope), `subscription` and `tenant`, as
`az account get-access-token` prints them, and is read for every call. The principal needs Owner
on a disposable subscription. Requires Docker and crane.
"""

import argparse
import base64
import datetime
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ARM = "https://management.azure.com"
CONTRIBUTOR = "b24988ac-6180-42a0-ab88-20f7382dd24c"
SECRETS_OFFICER = "b86a8fe4-44ce-4948-aee5-eccb2c155cd7"
APPS_API = "2024-03-01"
ACR_API = "2023-07-01"
MSI_API = "2023-01-31"
VAULT_API = "2023-07-01"
GROUP_API = "2021-04-01"
# alpine 3.20, linux/amd64: Container Apps runs nothing else, and a tag could resolve to a cached
# base of another platform.
BASE_IMAGE = "public.ecr.aws/docker/library/alpine@sha256:c64c687cbea9300178b30c95835354e34c4e4febc4badfe27102879de0483b5e"


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class Run:
    def __init__(self, args):
        self.args = args
        self.prefix = f"{args.prefix}{uuid.uuid4().hex[:4]}"
        self.tag_key, self.tag_value = args.tag.split("=", 1)
        self.tags = {self.tag_key: self.tag_value}
        token = self.token_file()
        self.subscription, self.tenant = token["subscription"], token["tenant"]
        self.location = args.location
        self.group = f"/subscriptions/{self.subscription}/resourceGroups/{self.prefix}"
        self.vault_name = f"{self.prefix}-kv"
        self.registry_name = self.prefix.replace("-", "") + "acr"
        self.registry = f"{self.registry_name}.azurecr.io"
        identities = f"{self.group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities"
        self.verifier_identity = f"{identities}/{self.prefix}-verifier"
        self.execution_identity = f"{identities}/{self.prefix}-execution-sa"
        apps = f"{self.group}/providers/Microsoft.App"
        self.environment = f"{apps}/managedEnvironments/{self.prefix}-default-container-env"
        self.verifier_environment = f"{apps}/managedEnvironments/{self.prefix}-verifier"
        self.verifier_app = f"{apps}/containerApps/{self.prefix}-verifier"
        self.worker_app = f"{apps}/containerApps/{self.prefix}-api"
        self.not_after = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=30)) \
            .replace(microsecond=0).isoformat()
        self.started = datetime.datetime.now(datetime.timezone.utc)
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="control-plane-azure-"))
        self.results = []

    # --- helpers ----------------------------------------------------------------

    def token_file(self):
        # The token file is replaced every few minutes; a read can land while it is rewritten.
        for attempt in range(30):
            try:
                return json.loads(pathlib.Path(self.args.token_file).read_text())
            except (OSError, ValueError):
                if attempt == 29:
                    raise
                time.sleep(2)

    def token(self):
        return self.token_file()["accessToken"]

    def arm(self, method, path, body=None, version=None, ok_missing=False, headers=False):
        url = path if path.startswith("https://") else ARM + path
        if version:
            url += ("&" if "?" in url else "?") + f"api-version={version}"
        request = urllib.request.Request(url, method=method,
                                         data=json.dumps(body).encode() if body is not None else None,
                                         headers={"Authorization": f"Bearer {self.token()}",
                                                  "content-type": "application/json"})
        try:
            with urllib.request.urlopen(request) as response:
                text = response.read()
                value = json.loads(text) if text else {}
                return (value, dict(response.headers)) if headers else value
        except urllib.error.HTTPError as error:
            text = error.read().decode()
            if error.code == 404 and ok_missing:
                return (None, {}) if headers else None
            raise RuntimeError(f"{method} {url}: HTTP {error.code}: {text[:1500]}") from None

    def provisioned(self, path, version, operation=None, minutes=20):
        for _ in range(minutes * 12):
            current = self.arm("GET", path, version=version)
            state = current.get("properties", {}).get("provisioningState")
            if state == "Succeeded":
                return current
            if state in ("Failed", "Canceled"):
                # The resource doesn't say why; the operation that wrote it does.
                reason = self.arm("GET", operation) if operation else current.get("properties")
                raise RuntimeError(f"{path}: {state}: {json.dumps(reason)[:3000]}")
            time.sleep(5)
        raise RuntimeError(f"{path}: not provisioned after {minutes} min")

    def put(self, path, version, body):
        _, headers = self.arm("PUT", path, body, version, headers=True)
        operation = {k.lower(): v for k, v in headers.items()}.get("azure-asyncoperation")
        return self.provisioned(path, version, operation)

    def assign(self, scope, role, principal):
        path = f"{scope}/providers/Microsoft.Authorization/roleAssignments/{uuid.uuid4()}"
        body = {"properties": {
            "roleDefinitionId": f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{role}",
            "principalId": principal, "principalType": "ServicePrincipal"}}
        for attempt in range(12):
            try:
                return self.arm("PUT", path, body, "2022-04-01")
            except RuntimeError as error:
                # A new identity takes a while to reach Entra ID.
                if attempt == 11 or "PrincipalNotFound" not in str(error):
                    raise
                time.sleep(10)

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    def driver(self, command, env=None, check=True):
        variables = {**os.environ, "PREFIX": self.prefix, "NOT_AFTER": self.not_after, "CLOUD": "azure",
                     "AZURE_TOKEN_FILE": self.args.token_file, "AZURE_LOCATION": self.location,
                     "ENVIRONMENT_NAME": self.environment.rsplit("/", 1)[-1],
                     "ENVIRONMENT_DOMAIN": getattr(self, "environment_domain", ""),
                     "EXECUTION_SA_PRINCIPAL_ID": getattr(self, "execution_principal", ""),
                     "EXECUTION_SA_CLIENT_ID": getattr(self, "execution_client", ""),
                     "DEPLOYMENT_TOKEN": getattr(self, "deployment_token", ""),
                     **(env or {})}
        result = subprocess.run([self.args.driver, command], capture_output=True, text=True, env=variables)
        (self.evidence / f"driver-{command}-{uuid.uuid4().hex[:4]}.log").write_text(
            result.stdout + "\n--- stderr ---\n" + result.stderr)
        if check and result.returncode != 0:
            raise RuntimeError(f"driver {command} failed: {result.stderr[-2000:]}")
        return result

    # --- setup ------------------------------------------------------------------

    def resource_group(self):
        self.arm("PUT", self.group, {"location": self.location, "tags": self.tags}, GROUP_API)
        log(f"resource group {self.prefix}")

    def identities(self):
        self.identity = {}
        for path in (self.verifier_identity, self.execution_identity):
            self.identity[path] = self.arm("PUT", path, {"location": self.location, "tags": self.tags},
                                           MSI_API)["properties"]
        self.execution_principal = self.identity[self.execution_identity]["principalId"]
        self.execution_client = self.identity[self.execution_identity]["clientId"]
        self.verifier_principal = self.identity[self.verifier_identity]["principalId"]

    def signing(self):
        """The settings tag and the ledger secret, as setup writes them."""
        fixtures = json.loads(self.driver("fixtures").stdout)
        self.put(f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}", VAULT_API, {
            "location": self.location, "tags": self.tags, "properties": {
                "tenantId": self.tenant, "sku": {"family": "A", "name": "standard"},
                "enableRbacAuthorization": True, "softDeleteRetentionInDays": 7,
                "publicNetworkAccess": "Enabled"}})
        self.arm("PUT", f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}/secrets/signing-state",
                 {"properties": {"value": json.dumps(fixtures["ledger"])}}, VAULT_API)
        settings = json.dumps(fixtures["settings"], separators=(",", ":"))
        self.arm("PATCH", self.group, {"tags": {**self.tags, "signing-fingerprint": settings}}, GROUP_API)
        self.assign(self.group, CONTRIBUTOR, self.verifier_principal)
        self.assign(f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}", SECRETS_OFFICER,
                    self.verifier_principal)

    def images(self):
        registry = f"{self.group}/providers/Microsoft.ContainerRegistry/registries/{self.registry_name}"
        self.put(registry, ACR_API, {"location": self.location, "tags": self.tags, "sku": {"name": "Basic"},
                                     "properties": {"adminUserEnabled": False}})
        # The manager's registry proxy takes user `deployment` and the deployment token; a
        # pull-only registry token of that name stands in for it.
        self.put(f"{registry}/tokens/deployment", ACR_API, {"properties": {
            "scopeMapId": f"{registry}/scopeMaps/_repositories_pull", "status": "enabled"}})
        credentials = self.arm("POST", f"{registry}/generateCredentials",
                               {"tokenId": f"{registry}/tokens/deployment", "name": "password1"}, ACR_API)
        self.deployment_token = credentials["passwords"][0]["value"]
        # Push with the caller's identity: an ACR refresh token for its ARM token.
        form = urllib.parse.urlencode({"grant_type": "access_token", "service": self.registry,
                                       "tenant": self.tenant, "access_token": self.token()}).encode()
        with urllib.request.urlopen(urllib.request.Request(f"https://{self.registry}/oauth2/exchange",
                                                           data=form)) as response:
            refresh = json.loads(response.read())["refresh_token"]
        login = ["login", self.registry, "-u", "00000000-0000-0000-0000-000000000000", "--password-stdin"]
        subprocess.run(["docker", *login], input=refresh.encode(), check=True, capture_output=True)
        subprocess.run(["crane", "auth", *login], input=refresh.encode(), check=True, capture_output=True)
        self.image = {}
        for tag, source in [("a", "nginxinc/nginx-unprivileged:1.27-alpine"),
                            ("b", "nginxinc/nginx-unprivileged:1.26-alpine")]:
            subprocess.run(["crane", "copy", "--platform", "linux/amd64", source, f"{self.registry}/api:{tag}"],
                           check=True, capture_output=True)
            digest = subprocess.run(["crane", "digest", f"{self.registry}/api:{tag}"], check=True,
                                    capture_output=True, text=True).stdout.strip()
            self.image[tag] = f"{self.registry}/api@{digest}"
        context = self.work / "verifier-image"
        context.mkdir()
        subprocess.run(["strip", "-o", str(context / "verifier"), self.args.verifier], check=True)
        (context / "Dockerfile").write_text(
            f"FROM {BASE_IMAGE}\nCOPY verifier /verifier\nENTRYPOINT [\"/verifier\"]\n")
        subprocess.run(["docker", "build", "--platform", "linux/amd64", "-t", f"{self.registry}/verifier:e2e",
                        str(context)], check=True, capture_output=True)
        subprocess.run(["docker", "push", f"{self.registry}/verifier:e2e"], check=True, capture_output=True)
        digest = subprocess.run(["crane", "digest", f"{self.registry}/verifier:e2e"], check=True,
                                capture_output=True, text=True).stdout.strip()
        self.verifier_image = f"{self.registry}/verifier@{digest}"
        # A new registry token takes a moment before it may pull.
        pull = {**os.environ, "DOCKER_CONFIG": str(self.work / "pull-check")}
        subprocess.run(["crane", "auth", "login", self.registry, "-u", "deployment", "--password-stdin"],
                       input=self.deployment_token.encode(), env=pull, check=True, capture_output=True)
        for _ in range(36):
            check = subprocess.run(["crane", "digest", self.verifier_image], env=pull, capture_output=True, text=True)
            if check.returncode == 0:
                break
            time.sleep(5)
        else:
            raise RuntimeError(f"the deployment token can't pull: {check.stderr[-500:]}")
        log(f"images pushed to {self.registry}")

    def environments(self):
        # Both environments setup renders, in the install's group.
        for path in (self.environment, self.verifier_environment):
            self.arm("PUT", path, {"location": self.location, "tags": self.tags,
                                   "properties": {"workloadProfiles": [
                                       {"name": "Consumption", "workloadProfileType": "Consumption"}]}}, APPS_API)
        for path in (self.environment, self.verifier_environment):
            self.provisioned(path, APPS_API)
        self.environment_domain = self.arm("GET", self.environment, version=APPS_API)["properties"]["defaultDomain"]
        log("environments ready")

    def verifier(self):
        env = {
            "VERIFIER_CLOUD": "azure", "RUST_LOG": "info",
            "VERIFIER_AZURE_RESOURCE_GROUP": self.group,
            "VERIFIER_AZURE_VAULT": f"https://{self.vault_name}.vault.azure.net",
            "VERIFIER_AZURE_CLIENT_ID": self.identity[self.verifier_identity]["clientId"],
            "VERIFIER_AZURE_ENVIRONMENT": self.environment,
        }
        app = self.put(self.verifier_app, APPS_API, {
            "location": self.location, "tags": self.tags,
            "identity": {"type": "UserAssigned", "userAssignedIdentities": {self.verifier_identity: {}}},
            "properties": {
                "environmentId": self.verifier_environment,
                "configuration": {
                    "ingress": {"external": True, "targetPort": 8080, "transport": "auto"},
                    "secrets": [{"name": "registry", "value": self.deployment_token}],
                    "registries": [{"server": self.registry, "username": "deployment",
                                    "passwordSecretRef": "registry"}],
                },
                "template": {
                    "containers": [{"name": "verifier", "image": self.verifier_image,
                                    "resources": {"cpu": 0.25, "memory": "0.5Gi"},
                                    "env": [{"name": k, "value": v} for k, v in env.items()]}],
                    "scale": {"minReplicas": 0, "maxReplicas": 1},
                },
            }})
        self.verifier_url = f"https://{app['properties']['configuration']['ingress']['fqdn']}/"
        log(f"verifier at {self.verifier_url}; waiting 60s for RBAC")
        time.sleep(60)

    def claims(self):
        payload = self.token().split(".")[1]
        return json.loads(base64.urlsafe_b64decode(payload + "=" * (-len(payload) % 4)))

    def allow(self, principal):
        """Container Apps authentication on the verifier, as setup renders it, letting `principal` in."""
        claims = self.claims()
        self.arm("PUT", f"{self.verifier_app}/authConfigs/current", version=APPS_API, body={"properties": {
            "platform": {"enabled": True},
            "globalValidation": {"unauthenticatedClientAction": "Return401"},
            "identityProviders": {"azureActiveDirectory": {
                "enabled": True,
                "registration": {"clientId": claims.get("appid", claims["oid"]),
                                 "openIdIssuer": f"https://sts.windows.net/{claims['tid']}/"},
                "validation": {"allowedAudiences": [ARM, ARM + "/"],
                               "defaultAuthorizationPolicy": {"allowedPrincipals": {"identities": [principal]}}}}}}})
        time.sleep(45)  # authentication settings reach the ingress

    def call_verifier(self, token):
        request = urllib.request.Request(self.verifier_url, method="POST", data=b"{}",
                                         headers={"content-type": "application/json",
                                                  **({"Authorization": f"Bearer {token}"} if token else {})})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.status, response.read().decode()
        except urllib.error.HTTPError as error:
            return error.code, error.read().decode()

    def authentication(self):
        management = self.claims()["oid"]
        self.allow(management)
        anonymous = self.call_verifier(None)
        allowed = self.call_verifier(self.token())
        self.allow(str(uuid.uuid4()))
        other = self.call_verifier(self.token())
        self.allow(management)
        reached = '"errorType"' in allowed[1]
        self.record("4 the verifier answers only management: no token 401, management reaches it, "
                    "another principal 403",
                    anonymous[0] == 401 and reached and allowed[0] != 401 and other[0] == 403
                    and '"errorType"' not in other[1],
                    f"no-token={anonymous[0]} management={allowed[0]} {allowed[1][:200]} "
                    f"other-principal={other[0]} {other[1][:100]}")

    # --- cases ------------------------------------------------------------------

    def app(self):
        return self.arm("GET", self.worker_app, version=APPS_API, ok_missing=True) or {}

    def worker_image(self):
        containers = self.app().get("properties", {}).get("template", {}).get("containers", [{}])
        return containers[0].get("image", "")

    def outcome(self, result):
        lines = result.stdout.strip().splitlines()
        try:
            return json.loads(lines[-1]) if lines else {}
        except ValueError:
            return {"stdout": lines[-1]}

    def cases(self):
        result = self.driver("update", {"FROM_IMAGE": "", "TO_IMAGE": self.image["a"], "PROTECTED": "1",
                                        "SEQUENCE": "1"}, check=False)
        outcome = self.outcome(result)
        app = self.app()
        properties = app.get("properties", {})
        identities = list(app.get("identity", {}).get("userAssignedIdentities", {}))
        ingress = properties.get("configuration", {}).get("ingress")
        (self.evidence / "worker-after-create.json").write_text(json.dumps(app, indent=2))
        self.record("0 the first deployment creates the worker's app through the verifier, in the stack's "
                    "environment, as the workload identity",
                    result.returncode == 0 and outcome.get("outcome") == "synced"
                    and self.worker_image() == self.image["a"]
                    and properties.get("environmentId", "").lower() == self.environment.lower()
                    and [i.lower() for i in identities] == [self.execution_identity.lower()]
                    and ingress is None,
                    f"driver={json.dumps(outcome)[:600]} image={self.worker_image()[-19:]} "
                    f"environment={properties.get('environmentId', '').rsplit('/', 1)[-1]} "
                    f"identities={[i.rsplit('/', 1)[-1] for i in identities]} ingress={ingress}")

        result = self.driver("update", {"FROM_IMAGE": self.image["a"], "TO_IMAGE": self.image["b"],
                                        "PROTECTED": "1", "SEQUENCE": "2"}, check=False)
        outcome = self.outcome(result)
        self.record("1 an update goes through the verifier",
                    result.returncode == 0 and outcome.get("outcome") == "synced"
                    and self.worker_image() == self.image["b"],
                    f"driver={json.dumps(outcome)[:600]} image={self.worker_image()[-19:]}")

        foreign_after = (datetime.datetime.now(datetime.timezone.utc) + datetime.timedelta(days=20)) \
            .replace(microsecond=0).isoformat()
        result = self.driver("update", {"FROM_IMAGE": self.image["b"], "TO_IMAGE": self.image["a"],
                                        "PROTECTED": "1", "SEQUENCE": "3", "NOT_AFTER": foreign_after},
                             check=False)
        outcome = self.outcome(result)
        self.record("2 a release under a signer list setup didn't pin is refused with the reason",
                    result.returncode != 0 and outcome.get("code") == "RELEASE_REFUSED_BY_VERIFIER"
                    and "does not match the pinned fingerprint" in json.dumps(outcome)
                    and self.worker_image() == self.image["b"],
                    f"driver={json.dumps(outcome)[:600]} image={self.worker_image()[-19:]}")

    def writers(self):
        """Who wrote the worker's app, from Azure's activity log (it lags by minutes)."""
        if not self.results[0]["pass"]:
            self.record("3 the activity log names only the verifier's identity as a writer of the "
                        "worker's app", False, "not checked: the worker was not created")
            return
        since = self.started.strftime("%Y-%m-%dT%H:%M:%SZ")
        query = urllib.parse.quote(f"eventTimestamp ge '{since}' and resourceGroupName eq '{self.prefix}'")
        url = (f"/subscriptions/{self.subscription}/providers/Microsoft.Insights/eventtypes/management/values"
               f"?$filter={query}")
        writes = []
        for _ in range(40):
            events = self.arm("GET", url, version="2015-04-01").get("value", [])
            writes = [e for e in events
                      if e.get("resourceId", "").lower() == self.worker_app.lower()
                      and e.get("operationName", {}).get("value", "").lower() == "microsoft.app/containerapps/write"
                      and e.get("status", {}).get("value") in ("Started", "Accepted", "Succeeded")]
            # Both successful writes (create and update) are in once two are logged.
            if len({e.get("correlationId") for e in writes if e["status"]["value"] == "Succeeded"}) >= 2:
                break
            time.sleep(30)
        (self.evidence / "worker-writes.json").write_text(json.dumps(writes, indent=2))
        callers = sorted({(e.get("caller"), e.get("claims", {}).get("http://schemas.microsoft.com/identity/claims/objectidentifier"))
                          for e in writes})
        succeeded = {e.get("correlationId") for e in writes if e["status"]["value"] == "Succeeded"}
        self.record("3 the activity log names only the verifier's identity as a writer of the worker's app",
                    len(succeeded) >= 2 and all(oid == self.verifier_principal for _, oid in callers),
                    f"writes={len(succeeded)} callers={callers} verifier={self.verifier_principal}")

    # --- cleanup ----------------------------------------------------------------

    def verifier_log(self):
        replicas = self.arm("GET", f"{self.verifier_app}/revisions", version=APPS_API, ok_missing=True) or {}
        (self.evidence / "verifier-revisions.json").write_text(json.dumps(replicas, indent=2))

    def cleanup(self):
        log("cleaning up")
        try:
            self.arm("DELETE", self.group, version=GROUP_API, ok_missing=True)
            for _ in range(180):
                if self.arm("GET", self.group, version=GROUP_API, ok_missing=True) is None:
                    break
                time.sleep(10)
        except Exception as error:
            log(f"resource group delete: {error}")
        deleted = (f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}"
                   f"/deletedVaults/{self.vault_name}")
        try:
            if self.arm("GET", deleted, version=VAULT_API, ok_missing=True):
                self.arm("POST", f"{deleted}/purge", version=VAULT_API)
                for _ in range(60):
                    if self.arm("GET", deleted, version=VAULT_API, ok_missing=True) is None:
                        break
                    time.sleep(10)
        except Exception as error:
            log(f"vault purge: {error}")
        subprocess.run(["docker", "logout", self.registry], capture_output=True)

    def audit(self):
        left = []
        if self.arm("GET", self.group, version=GROUP_API, ok_missing=True) is not None:
            left.append(f"resource group {self.prefix}")
        deleted = (f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}"
                   f"/deletedVaults/{self.vault_name}")
        if self.arm("GET", deleted, version=VAULT_API, ok_missing=True) is not None:
            left.append(f"deleted vault {self.vault_name}")
        groups = self.arm("GET", f"/subscriptions/{self.subscription}/resourcegroups", version=GROUP_API)["value"]
        left += [g["name"] for g in groups if g["name"].startswith(self.prefix)]
        self.record("cleanup: nothing named after the run is left", not left, f"left={left}")

    def run(self):
        log(f"run {self.prefix}")
        try:
            self.resource_group()
            self.identities()
            self.signing()
            self.images()
            self.environments()
            self.verifier()
            self.authentication()
            self.cases()
            self.writers()
        except Exception as error:
            self.record("the run itself", False, f"{type(error).__name__}: {error}")
        finally:
            try:
                self.verifier_log()
            except Exception as error:
                log(f"verifier revisions: {error}")
            self.cleanup()
            self.audit()
            (self.evidence / "results.json").write_text(json.dumps(self.results, indent=2))
        failed = [r for r in self.results if not r["pass"]]
        log(f"{len(self.results) - len(failed)}/{len(self.results)} passed")
        return 1 if failed else 0


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--verifier", required=True, help="a static (musl) build of alien-verifier")
    parser.add_argument("--driver", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--location", default="eastus")
    parser.add_argument("--prefix", default="e2e-cpz")
    parser.add_argument("--tag", default="purpose=control-plane-e2e")
    parser.add_argument("--evidence", required=True)
    sys.exit(Run(parser.parse_args()).run())


if __name__ == "__main__":
    main()
