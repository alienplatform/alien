#!/usr/bin/env python3
"""Check on real Azure what a protected setup's deny keeps from the management identity.

A protected Terraform module puts the install's resource group under a subscription deployment
stack whose deny settings refuse writes and deletes to all principals but the verifier and the
principal running setup. This check builds the same deployment stack around a group holding a
machine scale set (capacity 0, so nothing runs) and the signing vault, and tries, as a principal
that is not excluded (standing in for management, which never holds
`deploymentStacks/manageDenySetting/action`), what a compromised control plane would try.

Without the stack everything succeeds. With it, writing the group itself (where the
`signing-fingerprint` tag lives) and deleting it fail with DenyAssignmentAuthorizationFailed.
Azure does not carry the deny to the resources in the group: the deny assignment it creates
says `doNotApplyToChildScopes` even though the stack asks for child scopes. The check records
that the scale set's custom data, its scale, the tags extension and the vault stay writable to a
principal that holds the permissions, which is why protected management roles leave those
writes out (`alien_permissions::generators::verifier_owned::azure_changes_what_runs`).

Everything is named with --prefix and tagged with --tag, and deleted before exit; the run then
lists what still carries the prefix and fails if anything does.

  python3 scripts/azure-setup-locks/check.py --token-file token.json --prefix locks-check \\
      --tag purpose=locks-check --evidence /tmp/azure-locks

The token file holds `accessToken` (ARM scope) and `subscription`; it is read for every call.
"""

import argparse
import base64
import json
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

ARM = "https://management.azure.com"
FINGERPRINT = "SHA256:" + "9f2c3b1a77d0e5c2" * 4


class Run:
    def __init__(self, args):
        self.args = args
        self.subscription = json.loads(Path(args.token_file).read_text())["subscription"]
        self.location = args.location
        key, value = args.tag.split("=", 1)
        self.tags = {key: value}
        self.evidence = Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.results = []
        self.cleanups = []
        self.group = f"{args.prefix}-rg"

    def record(self, name, payload):
        (self.evidence / f"{name}.json").write_text(json.dumps(payload, indent=2, default=str) + "\n")

    def expect(self, variant, case, expected, actual, detail=None):
        ok = actual == expected
        self.results.append({"variant": variant, "case": case, "expected": expected, "actual": actual, "ok": ok})
        self.record(f"{variant}-{case}", {"expected": expected, "actual": actual, "detail": detail})
        print(f"{'ok  ' if ok else 'FAIL'} {variant:11} {case:34} expected {expected:34} got {actual}", flush=True)

    def call(self, method, path, body=None, api="2024-03-01"):
        url = f"{ARM}{path}{'&' if '?' in path else '?'}api-version={api}"
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        # Read for every call: the file is refreshed while a run waits on Azure.
        token = json.loads(Path(self.args.token_file).read_text())["accessToken"]
        request.add_header("Authorization", f"Bearer {token}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request) as response:
                text = response.read().decode()
                return response.status, json.loads(text) if text else {}, response.headers
        except urllib.error.HTTPError as error:
            text = error.read().decode()
            return error.code, json.loads(text) if text else {}, error.headers

    def wait(self, path, api, done):
        for _ in range(120):
            status, body, _ = self.call("GET", path, api=api)
            if done(status, body):
                return body
            time.sleep(10)
        raise SystemExit(f"timed out waiting for {path}")

    def code(self, status, body):
        if status < 300:
            return "Success"
        return body.get("error", {}).get("code", str(status))

    # Setup

    def build(self):
        sub = f"/subscriptions/{self.subscription}"
        rg = f"{sub}/resourceGroups/{self.group}"
        self.rg = rg
        status, body, _ = self.call("PUT", rg, {"location": self.location, "tags": {**self.tags, "signing-fingerprint": json.dumps({"fingerprint": FINGERPRINT, "prefix": self.args.prefix, "channel": "production"}, separators=(",", ":"))}}, api="2021-04-01")
        if status >= 300:
            raise SystemExit(f"create group: {body}")
        self.cleanups.append(self.delete_group)
        net = f"{rg}/providers/Microsoft.Network/virtualNetworks/{self.args.prefix}-vnet"
        status, body, _ = self.call("PUT", net, {"location": self.location, "tags": self.tags, "properties": {"addressSpace": {"addressPrefixes": ["10.70.0.0/16"]}, "subnets": [{"name": "machines", "properties": {"addressPrefix": "10.70.1.0/24"}}]}}, api="2023-09-01")
        if status >= 300:
            raise SystemExit(f"create vnet: {body}")
        self.wait(net, "2023-09-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded")
        with tempfile.TemporaryDirectory() as scratch:
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", f"{scratch}/key"], check=True)
            public_key = Path(f"{scratch}/key.pub").read_text().strip()
        self.vmss = f"{rg}/providers/Microsoft.Compute/virtualMachineScaleSets/{self.args.prefix}-machines"
        status, body, _ = self.call("PUT", self.vmss, {
            "location": self.location, "tags": self.tags,
            "sku": {"name": "Standard_B1s", "tier": "Standard", "capacity": 0},
            "properties": {
                "upgradePolicy": {"mode": "Manual"},
                "virtualMachineProfile": {
                    "osProfile": {
                        "computerNamePrefix": "m", "adminUsername": "machine",
                        "customData": base64.b64encode(b"signed boot").decode(),
                        "linuxConfiguration": {"disablePasswordAuthentication": True, "ssh": {"publicKeys": [{"path": "/home/machine/.ssh/authorized_keys", "keyData": public_key}]}},
                    },
                    "storageProfile": {"imageReference": {"publisher": "Canonical", "offer": "ubuntu-24_04-lts", "sku": "server", "version": "latest"}, "osDisk": {"createOption": "FromImage", "managedDisk": {"storageAccountType": "Standard_LRS"}}},
                    "networkProfile": {"networkInterfaceConfigurations": [{"name": "nic", "properties": {"primary": True, "ipConfigurations": [{"name": "ip", "properties": {"subnet": {"id": f"{net}/subnets/machines"}}}]}}]},
                },
            },
        }, api="2024-07-01")
        if status >= 300:
            raise SystemExit(f"create scale set: {body}")
        self.wait(self.vmss, "2024-07-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded")
        self.vault = f"{rg}/providers/Microsoft.KeyVault/vaults/{self.args.prefix}-sign"
        tenant = self.call("GET", f"/subscriptions/{self.subscription}", api="2022-12-01")[1]["tenantId"]
        self.tenant = tenant
        status, body, _ = self.call("PUT", self.vault, {"location": self.location, "tags": self.tags, "properties": {"tenantId": tenant, "sku": {"family": "A", "name": "standard"}, "enableRbacAuthorization": True, "softDeleteRetentionInDays": 7, "enablePurgeProtection": None}}, api="2023-07-01")
        if status >= 300:
            raise SystemExit(f"create vault: {body}")
        self.wait(self.vault, "2023-07-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded")
        self.cleanups.insert(0, self.purge_vault)  # After the group is gone.
        # The stand-in verifier: the deny's excluded principal.
        identity = f"{rg}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{self.args.prefix}-verifier"
        status, body, _ = self.call("PUT", identity, {"location": self.location, "tags": self.tags}, api="2023-01-31")
        if status >= 300:
            raise SystemExit(f"create identity: {body}")
        self.verifier_principal = body["properties"]["principalId"]
        time.sleep(60)  # A new identity takes a while to reach the directory deny settings name.

    def protect(self):
        """The deployment stack a protected module renders, around this group."""
        sub = f"/subscriptions/{self.subscription}"
        self.stack = f"{sub}/providers/Microsoft.Resources/deploymentStacks/{self.args.prefix}-protection"
        status, group, _ = self.call("GET", self.rg, api="2021-04-01")
        template = {
            "$schema": "https://schema.management.azure.com/schemas/2018-05-01/subscriptionDeploymentTemplate.json#",
            "contentVersion": "1.0.0.0",
            "resources": [{"type": "Microsoft.Resources/resourceGroups", "apiVersion": "2022-09-01", "name": self.group, "location": self.location, "tags": group["tags"]}],
        }
        body = {
            "location": self.location,
            "tags": self.tags,
            "properties": {
                "description": "Only the verifier writes what runs code in this install.",
                "actionOnUnmanage": {"resources": "detach", "resourceGroups": "detach", "managementGroups": "detach"},
                "denySettings": {"mode": "denyWriteAndDelete", "applyToChildScopes": True, "excludedPrincipals": [self.verifier_principal]},
                "template": template,
            },
        }
        status, response, _ = self.call("PUT", self.stack, body)
        if status >= 300:
            raise SystemExit(f"create deployment stack: {response}")
        self.cleanups.append(self.unprotect)
        done = self.wait(self.stack, "2024-03-01", lambda s, b: b.get("properties", {}).get("provisioningState") in ("succeeded", "Succeeded", "failed", "Failed"))
        self.record("deployment-stack", done)
        if done["properties"]["provisioningState"].lower() != "succeeded":
            raise SystemExit(f"deployment stack failed: {done['properties'].get('error')}")
        # Deny assignments reach authorization a few minutes after the stack succeeds. Write the
        # group unchanged until that is refused, and record how long it took.
        status, group, _ = self.call("GET", self.rg, api="2021-04-01")
        same = {"tags": group["tags"]}
        started = time.time()
        while True:
            status, body, _ = self.call("PATCH", self.rg, same, api="2021-04-01")
            if self.code(status, body) == "DenyAssignmentAuthorizationFailed":
                break
            if time.time() - started > 1800:
                raise SystemExit("the deny never applied to an unchanged tag write")
            time.sleep(30)
        self.record("deny-applied-after", {"seconds": round(time.time() - started)})
        status, assignments, _ = self.call("GET", f"{self.rg}/providers/Microsoft.Authorization/denyAssignments", api="2022-04-01")
        self.record("deny-assignments", assignments)

    def unprotect(self):
        status, body, _ = self.call("DELETE", f"{self.stack}?actionOnUnmanage=detachAll&bypassStackOutOfSyncError=true")
        self.wait(self.stack, "2024-03-01", lambda s, b: s == 404)

    def delete_group(self):
        self.call("DELETE", self.rg, api="2021-04-01")
        self.wait(self.rg, "2021-04-01", lambda s, b: s == 404)

    def purge_vault(self):
        name = self.vault.rsplit("/", 1)[1]
        path = f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}/deletedVaults/{name}"
        self.wait(path, "2023-07-01", lambda s, b: s == 200)
        self.call("POST", f"{path}/purge", api="2023-07-01")

    # Attempts

    def attempts(self, variant):
        protected = variant == "protected"
        denied = "DenyAssignmentAuthorizationFailed" if protected else "Success"
        settings = json.dumps({"fingerprint": "SHA256:" + "00" * 32, "prefix": self.args.prefix, "channel": "production"}, separators=(",", ":"))
        status, group, _ = self.call("GET", self.rg, api="2021-04-01")
        status, body, _ = self.call("PATCH", self.rg, {"tags": {**group["tags"], "signing-fingerprint": settings}}, api="2021-04-01")
        self.expect(variant, "change-settings-on-the-group", denied, self.code(status, body), body)
        if status < 300:
            self.call("PATCH", self.rg, {"tags": group["tags"]}, api="2021-04-01")
        # Not reached by the deny (see the module docs); the roles leave these out instead.
        status, body, _ = self.call("PATCH", f"{self.rg}/providers/Microsoft.Resources/tags/default", {"operation": "Merge", "properties": {"tags": {"probe": variant}}}, api="2021-04-01")
        self.expect(variant, "tags-extension-not-under-deny", "Success", self.code(status, body), body)
        status, body, _ = self.call("PATCH", self.vmss, {"properties": {"virtualMachineProfile": {"osProfile": {"customData": base64.b64encode(f"unsigned {variant}".encode()).decode()}}}}, api="2024-07-01")
        self.expect(variant, "scale-set-not-under-deny", "Success", self.code(status, body), body)
        if status < 300:
            self.wait(self.vmss, "2024-07-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded")
        status, body, _ = self.call("PATCH", self.vault, {"properties": {"enableRbacAuthorization": False}}, api="2023-07-01")
        self.expect(variant, "vault-not-under-deny", "Success", self.code(status, body), body)
        if status < 300:
            self.call("PATCH", self.vault, {"properties": {"enableRbacAuthorization": True}}, api="2023-07-01")
            self.wait(self.vault, "2023-07-01", lambda s, b: b.get("properties", {}).get("enableRbacAuthorization") is True)
        if protected:
            status, body, _ = self.call("DELETE", self.rg, api="2021-04-01")
            self.expect(variant, "delete-the-group", denied, self.code(status, body), body)

    def cleanup(self):
        failures = []
        for step in reversed(self.cleanups):
            try:
                step()
            except BaseException as error:  # Every step runs; any failure fails the run.
                failures.append(repr(error))
        self.cleanups.clear()
        return failures

    def audit(self):
        status, groups, _ = self.call("GET", f"/subscriptions/{self.subscription}/resourcegroups", api="2021-04-01")
        left = [g["name"] for g in groups.get("value", []) if g["name"].startswith(self.args.prefix)]
        status, stacks, _ = self.call("GET", f"/subscriptions/{self.subscription}/providers/Microsoft.Resources/deploymentStacks")
        left += [s["name"] for s in stacks.get("value", []) if s["name"].startswith(self.args.prefix)]
        self.record("cleanup-audit", {"left": left})
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--location", default="eastus")
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z][a-z0-9-]{2,14}", args.prefix):
        raise SystemExit("--prefix: 3 to 15 lowercase letters, digits and hyphens")
    run = Run(args)
    failures = []
    try:
        run.build()
        run.attempts("unprotected")
        run.protect()
        run.attempts("protected")
    finally:
        failures += [f"cleanup: {failure}" for failure in run.cleanup()]
        failures += [f"left behind: {item}" for item in run.audit()]
    failures += [f"{r['variant']} {r['case']}: expected {r['expected']}, got {r['actual']}" for r in run.results if not r["ok"]]
    run.record("summary", {"results": run.results, "failures": failures})
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(f"all {len(run.results)} checks passed; nothing left behind")


if __name__ == "__main__":
    main()
