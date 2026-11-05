#!/usr/bin/env python3
"""rollMachines, scale and removeMachines on real Azure, through the
verifier's own Azure code.

The verifier reads its ledger from Key Vault with its managed identity, so
the `verifier-local` example (the deployed verifier's stores, backend and
checks) runs on a small VM in the test resource group with a user-assigned
identity, behind a local stand-in for the Container Apps identity endpoint
that forwards to the VM's instance metadata. Each case is one Run Command.

Creates, in a new resource group `$TEST_RESOURCE_PREFIX-<random>` tagged
`<prefix>=true` (deleted at the end, the vault purged):
  - the signing settings as the resource-group tag and the ledger as the
    Key Vault secret, as setup writes them;
  - a network with setup's machine subnet and NSG, the machine identity
    `<p>-m` and the identity `<p>-workers-sa` setup creates for the signed
    stack's service account;
  - the verifier identity (Contributor on the group, Key Vault Secrets
    Officer on the vault, Reader on the subscription for gallery reads), a
    storage account holding the binary, and the runner VM.

The test build pins the Flatcar community gallery as the image publisher and
an oldest image date, so a current Flatcar image boots and an AKS gallery
image stands in for "not the publisher's". Each refusal is checked to write
nothing:
  1. a signed release rolls: scale set `<p>-compute-general` from the signed
     group (SKU, setup's subnet and NSG, the machine identity and the service
     account's, the machine identity's client id tag, upgrade policy Manual,
     no custom or user data), at the signed minimum, with a running machine;
  2. an image from another gallery is refused;
  3. a subnet or NSG that isn't setup's is refused (off: the NSG written
     straight into the scale set lands);
  4. a zone is refused;
  5. a publisher image older than the build's minimum is refused;
  6. a second roll writes a new model;
  7. the roll wrote the ledger back to Key Vault;
  8. `scale` resizes the scale set the verifier names within the signed
     bounds; a caller-named target or a zone is refused (off: a capacity past
     the maximum written straight to Azure lands);
  9. `removeMachines` deletes only members, never below the signed minimum
     (off: deleting the last machine directly lands);
  10. a disabled ledger secret refuses; enabled again, it works.

Usage (an ARM token JSON like `az account get-access-token` in --token-file):
    scripts/gate-e2e/azure_machines.py --verifier-local <musl example> --fixtures <gate-fixtures> \\
        --token-file <file> --evidence <dir>
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
import urllib.parse
import urllib.request
import uuid

ARM = "https://management.azure.com"
PUBLISHER = "flatcar-23485951-527a-48d6-9d11-6931ff0afc2e"
IMAGE = "flatcar-stable-amd64"
FOREIGN_GALLERY = "AKSUbuntu-38d80f77-467a-481f-a8d4-09b6d4220bd2"
FOREIGN_IMAGE = "2204gen2containerd"
MINIMUM_CREATED = "2024-06-01T00:00:00Z"
GALLERY_API = "2023-07-03"
VMSS_API = "2024-07-01"
ROLES = {
    "contributor": "b24988ac-6180-42a0-ab88-20f7382dd24c",
    "reader": "acdd72a7-3385-48ef-bd42-f606fba81ae7",
    "secrets-officer": "b86a8fe4-44ce-4948-aee5-eccb2c155cd7",
}

# The runner's stand-in for the Container Apps identity endpoint, and two
# Key Vault helpers that use the same identity.
RUNNER = r'''
import http.server, json, sys, urllib.parse, urllib.request

def token(resource, client_id=None):
    query = {"api-version": "2018-02-01", "resource": resource}
    if client_id:
        query["client_id"] = client_id
    request = urllib.request.Request("http://169.254.169.254/metadata/identity/oauth2/token?" + urllib.parse.urlencode(query),
                                     headers={"Metadata": "true"})
    return json.loads(urllib.request.urlopen(request).read())

class Identity(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        query = dict(urllib.parse.parse_qsl(urllib.parse.urlparse(self.path).query))
        if self.headers.get("X-IDENTITY-HEADER") != "local":
            self.send_response(401); self.end_headers(); return
        body = json.dumps(token(query["resource"], query.get("client_id"))).encode()
        self.send_response(200); self.send_header("content-type", "application/json"); self.end_headers(); self.wfile.write(body)

def vault(method, url, body=None):
    request = urllib.request.Request(url, method=method, data=json.dumps(body).encode() if body is not None else None,
                                     headers={"Authorization": "Bearer " + token("https://vault.azure.net")["access_token"],
                                              "content-type": "application/json"})
    return json.loads(urllib.request.urlopen(request).read())

if sys.argv[1] == "serve":
    http.server.HTTPServer(("127.0.0.1", 8099), Identity).serve_forever()
elif sys.argv[1] == "secret":
    print(json.dumps(vault("GET", f"{sys.argv[2]}/secrets/{sys.argv[3]}?api-version=7.4")))
elif sys.argv[1] == "enable":
    current = vault("GET", f"{sys.argv[2]}/secrets/{sys.argv[3]}/versions?api-version=7.4")["value"]
    newest = max(current, key=lambda v: v["attributes"]["created"])["id"]
    print(json.dumps(vault("PATCH", f"{newest}?api-version=7.4", {"attributes": {"enabled": sys.argv[4] == "true"}})["attributes"]))
'''


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class Run:
    def __init__(self, args):
        self.args = args
        label = os.environ.get("TEST_RESOURCE_PREFIX") or args.prefix
        if not label:
            raise SystemExit("set TEST_RESOURCE_PREFIX")
        self.label = label
        self.prefix = f"{label}-{uuid.uuid4().hex[:6]}"
        token = json.loads(pathlib.Path(args.token_file).read_text())
        self.subscription, self.tenant, self.location = token["subscription"], token["tenant"], args.location
        self.group = f"/subscriptions/{self.subscription}/resourceGroups/{self.prefix}"
        self.vault_name = f"{self.prefix}-kv"
        self.vault_url = f"https://{self.vault_name}.vault.azure.net"
        self.storage = self.prefix.replace("-", "") + "st"
        self.set_name = f"{self.prefix}-compute-general"
        self.set_path = f"{self.group}/providers/Microsoft.Compute/virtualMachineScaleSets/{self.set_name}"
        self.runner = f"{self.group}/providers/Microsoft.Compute/virtualMachines/{self.prefix}-runner"
        network = f"{self.group}/providers/Microsoft.Network"
        self.vnet = f"{network}/virtualNetworks/{self.prefix}-vnet"
        self.subnet = f"{self.vnet}/subnets/machines"
        self.nsg = f"{network}/networkSecurityGroups/{self.prefix}-nsg"
        self.other_nsg = f"{network}/networkSecurityGroups/{self.prefix}-open"
        identities = f"{self.group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities"
        self.machine_identity = f"{identities}/{self.prefix}-m"
        self.workers_identity = f"{identities}/{self.prefix}-workers-sa"
        self.verifier_identity = f"{identities}/{self.prefix}-verifier"
        self.subscription_assignments = []
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="azure-machines-"))
        self.results, self.calls = [], []

    # --- ARM ----------------------------------------------------------------

    def token(self):
        # The token file is replaced every few minutes; a read can land
        # while it is being rewritten.
        for attempt in range(30):
            try:
                return json.loads(pathlib.Path(self.args.token_file).read_text())["accessToken"]
            except (OSError, ValueError, KeyError):
                if attempt == 29:
                    raise
                time.sleep(2)

    def arm(self, method, path, body=None, version=None, ok_missing=False, raw=False):
        token = self.token()
        url = path if path.startswith("https://") else ARM + path
        if version:
            url += ("&" if "?" in url else "?") + f"api-version={version}"
        request = urllib.request.Request(url, method=method, data=json.dumps(body).encode() if body is not None else None,
                                         headers={"Authorization": f"Bearer {token}", "content-type": "application/json"})
        try:
            with urllib.request.urlopen(request) as response:
                text = response.read()
                value = json.loads(text) if text else {}
                return (value, dict(response.headers)) if raw else value
        except urllib.error.HTTPError as error:
            text = error.read().decode()
            if error.code == 404 and ok_missing:
                return (None, {}) if raw else None
            raise RuntimeError(f"{method} {url}: HTTP {error.code}: {text[:1500]}") from None

    def provisioned(self, path, version):
        for _ in range(180):
            current = self.arm("GET", path, version=version)
            state = current.get("properties", {}).get("provisioningState")
            if state == "Succeeded":
                return current
            if state in ("Failed", "Canceled"):
                raise RuntimeError(f"{path}: {state}")
            time.sleep(5)
        raise RuntimeError(f"{path}: not provisioned after 15 min")

    def put(self, path, version, body, retries=1):
        for attempt in range(retries):
            try:
                self.arm("PUT", path, body, version)
                break
            except RuntimeError as error:
                # A new identity takes a while to reach Entra ID.
                if attempt + 1 == retries or "PrincipalNotFound" not in str(error):
                    raise
                time.sleep(10)
        return self.provisioned(path, version) if "roleAssignments" not in path else None

    def assign(self, scope, role, principal):
        path = f"{scope}/providers/Microsoft.Authorization/roleAssignments/{uuid.uuid4()}"
        self.put(path, "2022-04-01", {"properties": {
            "roleDefinitionId": f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{ROLES[role]}",
            "principalId": principal, "principalType": "ServicePrincipal"}}, retries=12)
        return path

    def record(self, name, ok, detail):
        self.results.append({"case": name, "pass": bool(ok), "detail": detail})
        log(f"{name}: {'PASS' if ok else 'FAIL'} — {detail}")

    # --- setup ----------------------------------------------------------------

    def images(self):
        gallery = f"/subscriptions/{self.subscription}/providers/Microsoft.Compute/locations/{self.location}/communityGalleries"
        def versions(name, image):
            listed = self.arm("GET", f"{gallery}/{name}/images/{image}/versions", version=GALLERY_API)["value"]
            return sorted(listed, key=lambda v: v["properties"]["publishedDate"])
        flatcar = versions(PUBLISHER, IMAGE)
        self.image = flatcar[-1]["identifier"]["uniqueId"]
        old = [v for v in flatcar if v["properties"]["publishedDate"] < MINIMUM_CREATED]
        self.old_image = old[0]["identifier"]["uniqueId"]
        self.other_image = versions(FOREIGN_GALLERY, FOREIGN_IMAGE)[-1]["identifier"]["uniqueId"]

    def fixtures(self):
        out = self.work / "fixtures"
        subprocess.run([self.args.fixtures, str(out), self.prefix, f"azure:{self.subscription}:{self.prefix}",
                        "unused@sha256:" + "0" * 64, "unused@sha256:" + "1" * 64], check=True, capture_output=True)
        self.statements = {p.stem: json.loads(p.read_text()) for p in out.glob("*.json")}

    def setup(self):
        tags = {self.label: "true"}
        self.arm("PUT", self.group, {"location": self.location, "tags": tags}, "2021-04-01")
        log(f"resource group {self.prefix}")
        msi = "2023-01-31"
        self.identity = {}
        for path in (self.machine_identity, self.workers_identity, self.verifier_identity):
            self.identity[path] = self.arm("PUT", path, {"location": self.location, "tags": tags}, msi)["properties"]
        net = "2023-09-01"
        for nsg in (self.nsg, self.other_nsg):
            self.put(nsg, net, {"location": self.location, "tags": tags})
        self.put(self.vnet, net, {"location": self.location, "tags": tags, "properties": {
            "addressSpace": {"addressPrefixes": ["10.70.0.0/16"]},
            "subnets": [{"name": "machines", "properties": {"addressPrefix": "10.70.1.0/24"}},
                        {"name": "other", "properties": {"addressPrefix": "10.70.2.0/24"}},
                        {"name": "runner", "properties": {"addressPrefix": "10.70.3.0/24", "defaultOutboundAccess": True}}]}})
        self.put(f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}", "2023-07-01", {
            "location": self.location, "tags": tags, "properties": {
                "tenantId": self.tenant, "sku": {"family": "A", "name": "standard"}, "enableRbacAuthorization": True,
                "softDeleteRetentionInDays": 7, "publicNetworkAccess": "Enabled"}})
        # As setup writes them: the ledger secret and the settings tag.
        self.arm("PUT", f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}/secrets/signing-state",
                 {"properties": {"value": json.dumps(self.statements["ledger"])}}, "2023-07-01")
        settings = json.dumps(self.statements["settings"], separators=(",", ":"))
        self.arm("PATCH", self.group, {"tags": {**tags, "signing-fingerprint": settings}}, "2021-04-01")
        principal = self.identity[self.verifier_identity]["principalId"]
        self.assign(self.group, "contributor", principal)
        self.assign(f"{self.group}/providers/Microsoft.KeyVault/vaults/{self.vault_name}", "secrets-officer", principal)
        self.subscription_assignments.append(self.assign(f"/subscriptions/{self.subscription}", "reader", principal))
        self.upload()
        self.start_runner()

    def upload(self):
        path = f"{self.group}/providers/Microsoft.Storage/storageAccounts/{self.storage}"
        self.put(path, "2023-01-01", {"location": self.location, "tags": {self.label: "true"}, "sku": {"name": "Standard_LRS"},
                                      "kind": "StorageV2", "properties": {"allowBlobPublicAccess": False, "allowSharedKeyAccess": True,
                                                                          "minimumTlsVersion": "TLS1_2"}})
        self.arm("PUT", f"{path}/blobServices/default/containers/bin", {"properties": {}}, "2023-01-01")
        expiry = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(time.time() + 3 * 3600))
        sas = self.arm("POST", f"{path}/listAccountSas", {"signedServices": "b", "signedResourceTypes": "o", "signedPermission": "rcw",
                                                          "signedProtocol": "https", "signedExpiry": expiry}, "2023-01-01")["accountSasToken"]
        self.binary_url = f"https://{self.storage}.blob.core.windows.net/bin/verifier-local?{sas}"
        data = pathlib.Path(self.args.verifier_local).read_bytes()
        request = urllib.request.Request(self.binary_url, method="PUT", data=data,
                                         headers={"x-ms-blob-type": "BlockBlob", "x-ms-version": "2021-08-06"})
        urllib.request.urlopen(request).read()
        log(f"uploaded verifier-local ({len(data) // 1_000_000} MB)")

    def start_runner(self):
        key = self.work / "runner"
        subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)], check=True)
        nic = f"{self.group}/providers/Microsoft.Network/networkInterfaces/{self.prefix}-runner"
        self.put(nic, "2023-09-01", {"location": self.location, "tags": {self.label: "true"}, "properties": {
            "ipConfigurations": [{"name": "primary", "properties": {"subnet": {"id": f"{self.vnet}/subnets/runner"}}}]}})
        last = None
        for size in ("Standard_D2s_v3",):
            try:
                self.put(self.runner, VMSS_API, {
                    "location": self.location, "tags": {self.label: "true"},
                    "identity": {"type": "UserAssigned", "userAssignedIdentities": {self.verifier_identity: {}}},
                    "properties": {
                        "hardwareProfile": {"vmSize": size},
                        "storageProfile": {"imageReference": {"publisher": "Canonical", "offer": "ubuntu-24_04-lts",
                                                              "sku": "server", "version": "latest"},
                                           "osDisk": {"createOption": "FromImage", "deleteOption": "Delete",
                                                      "managedDisk": {"storageAccountType": "StandardSSD_LRS"}}},
                        "osProfile": {"computerName": "runner", "adminUsername": "runner", "linuxConfiguration": {
                            "disablePasswordAuthentication": True,
                            "ssh": {"publicKeys": [{"path": "/home/runner/.ssh/authorized_keys",
                                                    "keyData": key.with_suffix(".pub").read_text().strip()}]}}},
                        "networkProfile": {"networkInterfaces": [{"id": nic, "properties": {"deleteOption": "Delete"}}]}}})
                log(f"runner VM {size}")
                break
            except RuntimeError as error:
                last = error
                log(f"runner {size}: {str(error)[:300]}")
                self.arm("DELETE", self.runner, version=VMSS_API, ok_missing=True)
        else:
            raise last
        program = base64.b64encode(RUNNER.encode()).decode()
        out = self.run_command([
            "mkdir -p /opt/v",
            f"echo {program} | base64 -d > /opt/v/runner.py",
            f"curl -fsS -o /opt/v/verifier-local '{self.binary_url}'",
            "chmod +x /opt/v/verifier-local",
            "systemd-run --unit verifier-identity python3 /opt/v/runner.py serve",
            "sleep 2; curl -fsS -H 'X-IDENTITY-HEADER: local' 'http://127.0.0.1:8099/token?resource=https://management.azure.com/' | head -c 20",
        ])
        log(f"runner ready: {out[:200]}")

    def run_command(self, script):
        _, headers = self.arm("POST", f"{self.runner}/runCommand", {"commandId": "RunShellScript", "script": script},
                              VMSS_API, raw=True)
        headers = {key.lower(): value for key, value in headers.items()}
        status = headers.get("azure-asyncoperation") or headers.get("location")
        if not status:
            raise RuntimeError(f"run command: no operation to follow in {sorted(headers)}")
        for _ in range(120):
            time.sleep(4)
            result = self.arm("GET", status)
            if result.get("status") in ("Succeeded", "Failed", "Canceled"):
                break
            if "value" in result:
                break
        if result.get("status") in ("Failed", "Canceled"):
            raise RuntimeError(f"run command: {json.dumps(result)[:1000]}")
        message = (result.get("properties", {}).get("output") or result).get("value", [{}])[0].get("message", "")
        return message.split("[stdout]\n", 1)[-1].split("\n[stderr]", 1)[0]

    # --- calls ------------------------------------------------------------

    def invoke(self, kind="rollMachines", **overrides):
        base = {"cluster": "compute", "group": "general", "release": self.statements["release-72-machines-azure"],
                "signerLists": [self.statements["signer-list"]]}
        if kind == "rollMachines":
            base["machineImage"] = self.image
        request = {kind: {**base, **overrides}}
        network = json.dumps({"subnetIds": [self.subnet], "securityGroupIds": [self.nsg], "identity": self.machine_identity})
        encoded = base64.b64encode(json.dumps(request).encode()).decode()
        out = self.run_command([
            f"echo {encoded} | base64 -d > /tmp/request.json",
            f"IDENTITY_ENDPOINT=http://127.0.0.1:8099/token IDENTITY_HEADER=local /opt/v/verifier-local azure {self.group} "
            f"{self.vault_url} {self.prefix}-verifier {PUBLISHER} {MINIMUM_CREATED} '{network}' < /tmp/request.json > /tmp/answer.json 2>/tmp/log.txt; "
            "echo \"exit=$?\"; cat /tmp/answer.json; tail -c 600 /tmp/log.txt",
        ])
        lines = out.strip().splitlines()
        exit_line = next((l for l in lines if l.startswith("exit=")), "exit=?")
        answer_line = next((l for l in lines if l.startswith("{")), "{}")
        try:
            body = json.loads(answer_line)
        except json.JSONDecodeError:
            body = {"errorMessage": out[-500:]}
        ok = exit_line == "exit=0"
        self.calls.append({"request": {kind: {k: v for k, v in request[kind].items() if k not in ("release", "signerLists")}},
                           "ok": ok, "response": body, "output": out[-1500:]})
        return ok, body if ok else body.get("errorMessage", out[-500:])

    def scale_set(self):
        return self.arm("GET", self.set_path, version=VMSS_API, ok_missing=True)

    def members(self):
        listed = self.arm("GET", f"{self.set_path}/virtualMachines", version=VMSS_API, ok_missing=True) or {}
        return sorted(vm["instanceId"] for vm in listed.get("value", []))

    def capacity(self):
        return self.scale_set()["sku"]["capacity"]

    def settle(self, size):
        for _ in range(90):
            current = self.scale_set()
            if current["properties"]["provisioningState"] == "Succeeded" and len(self.members()) == size:
                return True
            time.sleep(5)
        return False

    def ledger(self):
        out = self.run_command([f"python3 /opt/v/runner.py secret {self.vault_url} signing-state"])
        return json.loads(json.loads(out.strip().splitlines()[-1])["value"])

    def expect_refused(self, name, reason, **overrides):
        before = self.scale_set()
        ok, body = self.invoke(**overrides)
        after = self.scale_set()
        unchanged = (before or {}).get("etag") == (after or {}).get("etag")
        self.record(name, not ok and reason in str(body) and unchanged, f"ok={ok} unchanged={unchanged} reason={str(body)[:400]}")

    def run(self):
        self.images()
        self.fixtures()
        self.setup()
        log(f"ready: prefix {self.prefix}, image {self.image}")

        self.expect_refused("2 an image from another gallery (on)", f"not the publisher {PUBLISHER}", machineImage=self.other_image)
        self.expect_refused("3 a subnet that isn't setup's (on)", "is not one setup created", subnetIds=[f"{self.vnet}/subnets/other"])
        self.expect_refused("3 an NSG that isn't setup's (on)", "is not one setup created", securityGroupIds=[self.other_nsg])
        self.expect_refused("4 a zone (on)", "a scale set takes no zone", zone="1")
        self.expect_refused("5 a publisher image older than the minimum (on)", "the oldest image this build boots", machineImage=self.old_image)
        self.record("2-5 nothing was created", self.scale_set() is None, "no scale set")

        ok, body = self.invoke()
        current = self.scale_set() or {}
        self.record("1 a signed release rolls", ok and body.get("group") == self.set_name and body.get("template") == self.set_name
                    and body.get("version") == current.get("etag"), str(body)[:300])
        profile = current.get("properties", {}).get("virtualMachineProfile", {})
        interface = profile.get("networkProfile", {}).get("networkInterfaceConfigurations", [{}])[0].get("properties", {})
        identities = {key.lower() for key in current.get("identity", {}).get("userAssignedIdentities", {})}
        os_profile = profile.get("osProfile", {})
        self.record("1 the scale set is the signed group's", current.get("sku", {}).get("name") == "Standard_D2s_v3"
                    and profile["storageProfile"]["imageReference"].get("communityGalleryImageId") == self.image
                    and interface["ipConfigurations"][0]["properties"]["subnet"]["id"].lower() == self.subnet.lower()
                    and interface.get("networkSecurityGroup", {}).get("id", "").lower() == self.nsg.lower()
                    and identities == {self.machine_identity.lower(), self.workers_identity.lower()}
                    and current.get("tags", {}).get("machine-identity-client-id") == self.identity[self.machine_identity]["clientId"]
                    and current["properties"]["upgradePolicy"]["mode"] == "Manual"
                    and "customData" not in os_profile and "userData" not in profile
                    and os_profile["linuxConfiguration"]["disablePasswordAuthentication"] is True,
                    json.dumps({"sku": current.get("sku"), "tags": current.get("tags"), "identities": sorted(identities),
                                "upgradePolicy": current.get("properties", {}).get("upgradePolicy")})[:700])
        self.record("1 it starts at the signed minimum with a running machine", current.get("sku", {}).get("capacity") == 1
                    and self.settle(1), json.dumps(self.members()))

        # 3 off: the same foreign NSG written straight into the scale set lands.
        model = self.scale_set()
        model["properties"]["virtualMachineProfile"]["networkProfile"]["networkInterfaceConfigurations"][0]["properties"]["networkSecurityGroup"] = {"id": self.other_nsg}
        self.arm("PATCH", self.set_path, {"properties": {"virtualMachineProfile": {"networkProfile": model["properties"]["virtualMachineProfile"]["networkProfile"]}}}, VMSS_API)
        written = self.provisioned(self.set_path, VMSS_API)
        nsg = written["properties"]["virtualMachineProfile"]["networkProfile"]["networkInterfaceConfigurations"][0]["properties"]["networkSecurityGroup"]["id"]
        self.record("3 an NSG that isn't setup's (off: written straight to Azure)", nsg.lower() == self.other_nsg.lower(), "landed")

        etag = self.scale_set()["etag"]
        ok, body = self.invoke()
        current = self.scale_set()
        nsg = current["properties"]["virtualMachineProfile"]["networkProfile"]["networkInterfaceConfigurations"][0]["properties"]["networkSecurityGroup"]["id"]
        self.record("6 a second roll writes a new model (and puts setup's NSG back)", ok and body.get("version") == current["etag"]
                    and current["etag"] != etag and nsg.lower() == self.nsg.lower(), str(body)[:300])

        ledger = self.ledger()
        self.record("7 the roll wrote the ledger back to Key Vault", ledger.get("release", {}).get("sequence") == 72, json.dumps(ledger)[:300])

        ok, body = self.invoke("scale", desired=5)
        self.record("8 scale to 5 resizes to the signed maximum 2 (on)", ok and body == {"target": self.set_name, "size": 2}
                    and self.capacity() == 2, str(body)[:300])
        ok, body = self.invoke("scale", desired=0)
        self.record("8 scale to 0 resizes to the signed minimum 1 (on)", ok and body.get("size") == 1 and self.capacity() == 1, str(body)[:300])
        ok, body = self.invoke("scale", desired=2, target=f"{self.prefix}-other")
        self.record("8 a caller-named scale target is refused (on)", not ok and "unknown field `target`" in str(body) and self.capacity() == 1, str(body)[:300])
        ok, body = self.invoke("scale", desired=2, zone="1")
        self.record("8 a scale zone is refused (on)", not ok and "zone" in str(body) and self.capacity() == 1, str(body)[:300])
        self.arm("PATCH", self.set_path, {"sku": {"capacity": 3}}, VMSS_API)
        self.provisioned(self.set_path, VMSS_API)
        self.record("8 a capacity past the signed maximum (off: written straight to Azure)", self.capacity() == 3, "landed: capacity 3")

        ok, body = self.invoke("scale", desired=2)
        settled = self.settle(2)
        members = self.members()
        self.record("9 two machines to remove from", ok and settled and len(members) == 2, json.dumps(members))
        ok, body = self.invoke("removeMachines", machines=["999"])
        self.record("9 removing a machine that isn't a member is refused (on)", not ok and "is not a member" in str(body)
                    and self.members() == members, str(body)[:300])
        ok, body = self.invoke("removeMachines", machines=members)
        self.record("9 removing both machines (below the signed minimum 1) is refused (on)", not ok and "minimum" in str(body)
                    and self.members() == members, str(body)[:300])
        ok, body = self.invoke("removeMachines", machines=members[:1])
        gone = self.settle(1) and self.members() == members[1:]
        self.record("9 removing one member lands", ok and body == {"group": self.set_name, "removed": members[:1], "size": 1} and gone,
                    json.dumps({"body": body, "members": self.members()}))
        ok, body = self.invoke("removeMachines", machines=members[1:])
        self.record("9 removing the last machine is refused (on)", not ok and "minimum" in str(body) and self.members() == members[1:], str(body)[:300])
        self.arm("POST", f"{self.set_path}/delete", {"instanceIds": members[1:]}, VMSS_API)
        self.settle(0)
        self.record("9 removing the last machine (off: written straight to Azure)", self.capacity() == 0 and self.members() == [], "landed: capacity 0")

        self.run_command([f"python3 /opt/v/runner.py enable {self.vault_url} signing-state false"])
        ok, body = self.invoke("scale", desired=1)
        self.record("10 a disabled ledger secret refuses (on)", not ok and "signing-state" in str(body) and self.capacity() == 0, str(body)[:400])
        self.run_command([f"python3 /opt/v/runner.py enable {self.vault_url} signing-state true"])
        ok, body = self.invoke("scale", desired=1)
        self.record("10 enabled again, scale lands", ok and body.get("size") == 1, str(body)[:300])

    # --- teardown -------------------------------------------------------------

    def cleanup(self):
        log("cleaning up")
        for path in self.subscription_assignments:
            try:
                self.arm("DELETE", path, version="2022-04-01", ok_missing=True)
            except Exception as error:
                log(f"role assignment {path}: {error}")
        try:
            self.arm("DELETE", self.group, version="2021-04-01", ok_missing=True)
            for _ in range(180):
                if self.arm("GET", self.group, version="2021-04-01", ok_missing=True) is None:
                    break
                time.sleep(10)
        except Exception as error:
            log(f"resource group delete: {error}")
        deleted = f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}/deletedVaults/{self.vault_name}"
        try:
            if self.arm("GET", deleted, version="2023-07-01", ok_missing=True):
                self.arm("POST", f"{deleted}/purge", version="2023-07-01")
                for _ in range(60):
                    if self.arm("GET", deleted, version="2023-07-01", ok_missing=True) is None:
                        break
                    time.sleep(10)
        except Exception as error:
            log(f"vault purge: {error}")

    def audit(self):
        left = []
        if self.arm("GET", self.group, version="2021-04-01", ok_missing=True) is not None:
            left.append(f"resource group {self.prefix}")
        for path in self.subscription_assignments:
            if self.arm("GET", path, version="2022-04-01", ok_missing=True) is not None:
                left.append(f"role assignment {path}")
        deleted = f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}/deletedVaults/{self.vault_name}"
        if self.arm("GET", deleted, version="2023-07-01", ok_missing=True) is not None:
            left.append(f"deleted vault {self.vault_name}")
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--verifier-local", required=True, help="a static (musl) build of the verifier-local example")
    parser.add_argument("--fixtures", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--location", default="centralus")
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
