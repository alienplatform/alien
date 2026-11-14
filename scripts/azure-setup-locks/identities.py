#!/usr/bin/env python3
"""Check on real Azure what the roles a setup gives each identity let it do.

Renders `scripts/setup-render-diff/fixtures/containers.json` as Terraform for Azure with the
`render_setup` example of alien-cli-common (`cargo build -p alien-cli-common --example
render_setup`), once unprotected and once protected, and reads from each module the role
definitions setup assigns to the management identity (and, protected, to the verifier and to the
machines through `azure_machine_signing_grants`).

For each variant it builds a resource group holding what those roles act on: a machine scale set
(capacity 0, so nothing runs), the signing vault with a `signing-state` secret and, protected,
the `signing-fingerprint` tag. It creates one managed identity per role holder, gives it exactly
the rendered roles (same actions, data actions and assignment conditions) and runs a container
instance as that identity which tries, through the Azure APIs:

  * management: the group's tags through Microsoft.Resources/tags, the group itself, the scale
    set's custom data, its capacity, a new scale set, a secret write, a role assignment and a
    write on the signing vault;
  * verifier (protected): the scale set's capacity;
  * machines (protected, and an identity without the grants as the control): the group's tags,
    the vault list, the identities, the ledger secret read and write.

Each case expects Success exactly when the rendered roles hold the action, so the unprotected
management identity shows what the protected one lost, and the protected one must be refused
every write of what runs code and every write of the signing settings.

Everything is named with --prefix and tagged with --tag, and deleted before exit (custom role
definitions too, and the vaults are purged); the run then lists what still carries the prefix and
fails if anything does.

  python3 scripts/azure-setup-locks/identities.py --renderer target/debug/examples/render_setup \\
      --token-file token.json --prefix locks-id --tag purpose=locks-check --evidence /tmp/ids

The token file holds `accessToken` (ARM scope) and `subscription`; it is read for every call.
The principal running this needs Owner on the subscription.
"""

import argparse
import base64
import fnmatch
import json
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path

ARM = "https://management.azure.com"
HERE = Path(__file__).resolve().parent
FIXTURE = HERE.parent / "setup-render-diff" / "fixtures" / "containers.json"
FINGERPRINT = "SHA256:" + "9f2c3b1a77d0e5c2" * 4
READER = "acdd72a7-3385-48ef-bd42-f606fba81ae7"
SECRETS_OFFICER = "b86a8fe4-44ce-4948-aee5-eccb2c155cd7"
IMAGE = "mcr.microsoft.com/azure-cli:2.67.0"
MANAGEMENT = "azurerm_user_assigned_identity.management.principal_id"
VERIFIER = "azurerm_user_assigned_identity.management_verifier.principal_id"

# Run inside the container as the identity: one JSON line per attempt.
PROBE = r'''
import json, os, subprocess, urllib.request, urllib.error
def token(resource):
    out = subprocess.run(["az", "account", "get-access-token", "--resource", resource, "-o", "json"], capture_output=True, text=True, check=True)
    return json.loads(out.stdout)["accessToken"]
import time
# The identity endpoint of a container instance answers a while after the container starts.
for attempt in range(30):
    login = subprocess.run(["az", "login", "--identity", "--username", os.environ["CLIENT_ID"], "--allow-no-subscriptions", "-o", "none"])
    if login.returncode == 0:
        break
    if attempt == 29:
        raise SystemExit("az login --identity kept failing")
    time.sleep(10)
tokens = {}
for case in json.loads(os.environ["CASES"]):
    resource = case.get("resource", "https://management.azure.com")
    if resource not in tokens:
        tokens[resource] = token(resource)
    request = urllib.request.Request(case["url"], data=json.dumps(case["body"]).encode() if "body" in case else None, method=case["method"])
    request.add_header("Authorization", "Bearer " + tokens[resource])
    request.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(request) as response:
            result = {"status": response.status, "code": "Success"}
    except urllib.error.HTTPError as error:
        text = error.read().decode()
        try:
            body = json.loads(text)
        except ValueError:
            body = {"text": text}
        code = (body.get("error") or {}).get("code") or str(error.code)
        result = {"status": error.code, "code": code, "message": (body.get("error") or {}).get("message")}
    print("RESULT " + json.dumps({"case": case["name"], **result}), flush=True)
print("DONE", flush=True)
'''

DENIED = {"AuthorizationFailed", "Forbidden", "LinkedAuthorizationFailed", "DenyAssignmentAuthorizationFailed"}


def denied(code):
    return "Denied" if code in DENIED else code


def parse_roles(module):
    """Role definitions and role assignments of a rendered module, by Terraform label."""
    text = "\n".join(module.values())
    definitions = {}
    for match in re.finditer(r'resource "azurerm_role_definition" "([^"]+)" \{\n(.*?)\n\}\n', text, re.S):
        body = match.group(2)

        def listed(key):
            found = re.search(rf"\b{key}\s*=\s*\[(.*?)\]", body, re.S)
            return re.findall(r'"([^"]+)"', found.group(1)) if found else []

        seed = re.search(r'role_definition_id\s*=\s*uuidv5\("oid", "([^"]+)"\)', body)
        definitions[match.group(1)] = {"actions": listed("actions"), "data_actions": listed("data_actions"), "seed": seed.group(1) if seed else None}
    assignments = []
    for match in re.finditer(r'resource "azurerm_role_assignment" "([^"]+)" \{\n(.*?)\n\}\n', text, re.S):
        body = match.group(2)
        principal = re.search(r"principal_id\s*=\s*(.+)", body).group(1).strip()
        role = re.search(r"role_definition_id\s*=\s*(.+)", body)
        role = role.group(1).strip() if role else ""
        definition = re.search(r"azurerm_role_definition\.([A-Za-z0-9_]+)\.", role)
        predefined = re.search(r"roleDefinitions/([0-9a-f-]{36})", role)
        if re.search(r'role_definition_name\s*=\s*"Reader"', body):
            predefined = re.match(r"(.*)", READER)
        condition = re.search(r'condition\s*=\s*"(.*)"\n', body)
        assignments.append({
            "label": match.group(1),
            "principal": principal,
            "definition": definition.group(1) if definition else None,
            "predefined": predefined.group(1) if predefined else None,
            "condition": condition.group(1).replace('\\"', '"') if condition else None,
        })
    runtime_roles = {}
    found = re.search(r"management_runtime_role_ids\s*=\s*\{(.*?)\n\s*\}", text, re.S)
    if found:
        for key, seed in re.findall(r'(r\d+)\s*=\s*uuidv5\("oid", "([^"]+)"\)', found.group(1)):
            label = next((label for label, d in definitions.items() if d["seed"] == seed), None)
            if label is None:
                raise SystemExit(f"runtime role {key} names no role definition of the module")
            runtime_roles[key] = label
    return definitions, assignments, runtime_roles


def holds(actions, action):
    return any(fnmatch.fnmatch(action.lower(), pattern.lower()) for pattern in actions)


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
        self.groups = []
        self.role_definitions = []
        self.vaults = []

    def record(self, name, payload):
        (self.evidence / f"{name}.json").write_text(json.dumps(payload, indent=2, default=str) + "\n")

    def expect(self, variant, case, expected, actual, detail=None):
        ok = actual == expected
        self.results.append({"variant": variant, "case": case, "expected": expected, "actual": actual, "ok": ok})
        self.record(f"{variant}-{case}", {"expected": expected, "actual": actual, "detail": detail})
        print(f"{'ok  ' if ok else 'FAIL'} {variant:22} {case:28} expected {expected:8} got {actual}", flush=True)

    def call(self, method, path, body=None, api="2022-04-01"):
        url = path if path.startswith("https://") else f"{ARM}{path}"
        url = f"{url}{'&' if '?' in url else '?'}api-version={api}"
        data = json.dumps(body).encode() if body is not None else None
        request = urllib.request.Request(url, data=data, method=method)
        # Read for every call: the file is refreshed while a run waits on Azure.
        token = json.loads(Path(self.args.token_file).read_text())["accessToken"]
        request.add_header("Authorization", f"Bearer {token}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request) as response:
                text = response.read().decode()
                return response.status, json.loads(text) if text else {}
        except urllib.error.HTTPError as error:
            text = error.read().decode()
            try:
                return error.code, json.loads(text) if text else {}
            except ValueError:
                return error.code, {"text": text}

    def must(self, method, path, body=None, api="2022-04-01"):
        status, response = self.call(method, path, body, api)
        if status >= 300:
            raise SystemExit(f"{method} {path}: {status} {response}")
        return response

    def wait(self, path, api, done, what):
        for _ in range(90):
            status, body = self.call("GET", path, api=api)
            if done(status, body):
                return body
            time.sleep(10)
        raise SystemExit(f"timed out waiting for {what}")

    # Rendering

    def render(self, scratch, protected):
        settings = {}
        if protected:
            settings["signing"] = {"fingerprint": FINGERPRINT, "channel": "production"}
        settings_file = Path(scratch) / f"settings-{protected}.json"
        settings_file.write_text(json.dumps(settings))
        out = Path(scratch) / f"module-{protected}.json"
        command = [str(self.args.renderer), "--stub-compute-clusters", "terraform-azure", str(FIXTURE), str(settings_file), str(out)]
        if protected:
            command += ["111122223333", "verifier@sha256:" + "cd" * 32]
        subprocess.run(command, check=True)
        return json.loads(out.read_text())

    # Building

    def build_group(self, variant, protected):
        group = f"{self.args.prefix}-{variant[0]}"
        rg = f"/subscriptions/{self.subscription}/resourceGroups/{group}"
        tags = dict(self.tags)
        if protected:
            tags["signing-fingerprint"] = json.dumps({"fingerprint": FINGERPRINT, "prefix": group, "channel": "production"}, separators=(",", ":"))
        self.must("PUT", rg, {"location": self.location, "tags": tags}, api="2021-04-01")
        self.groups.append(rg)
        net = f"{rg}/providers/Microsoft.Network/virtualNetworks/{group}-vnet"
        self.must("PUT", net, {"location": self.location, "tags": self.tags, "properties": {"addressSpace": {"addressPrefixes": ["10.71.0.0/16"]}, "subnets": [{"name": "machines", "properties": {"addressPrefix": "10.71.1.0/24"}}]}}, api="2023-09-01")
        self.wait(net, "2023-09-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded", "vnet")
        vmss = f"{rg}/providers/Microsoft.Compute/virtualMachineScaleSets/{group}-machines"
        self.must("PUT", vmss, self.scale_set_body(net), api="2024-07-01")
        self.wait(vmss, "2024-07-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded", "scale set")
        tenant = self.must("GET", f"/subscriptions/{self.subscription}", api="2022-12-01")["tenantId"]
        vault_name = f"{group}-sign-{uuid.uuid4().hex[:4]}"
        vault = f"{rg}/providers/Microsoft.KeyVault/vaults/{vault_name}"
        body = self.must("PUT", vault, {"location": self.location, "tags": self.tags, "properties": {"tenantId": tenant, "sku": {"family": "A", "name": "standard"}, "enableRbacAuthorization": True, "softDeleteRetentionInDays": 7}}, api="2023-07-01")
        self.vaults.append(vault_name)
        body = self.wait(vault, "2023-07-01", lambda s, b: b.get("properties", {}).get("provisioningState") == "Succeeded", "vault")
        return {"group": group, "rg": rg, "net": net, "vmss": vmss, "vault": vault, "vault_uri": body["properties"]["vaultUri"].rstrip("/"), "tenant": tenant}

    def scale_set_body(self, net):
        with tempfile.TemporaryDirectory() as scratch:
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", f"{scratch}/key"], check=True)
            public_key = Path(f"{scratch}/key.pub").read_text().strip()
        return {
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
        }

    def identity(self, place, name):
        path = f"{place['rg']}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{place['group']}-{name}"
        body = self.must("PUT", path, {"location": self.location, "tags": self.tags}, api="2023-01-31")
        return {"id": path, "principal": body["properties"]["principalId"], "client": body["properties"]["clientId"]}

    def role(self, place, label, definition):
        role_id = str(uuid.uuid5(uuid.NAMESPACE_URL, f"{place['rg']}/{label}"))
        path = f"{place['rg']}/providers/Microsoft.Authorization/roleDefinitions/{role_id}"
        self.must("PUT", path, {"properties": {
            "roleName": f"{place['group']}-{label}"[:128], "type": "CustomRole",
            "description": f"Rendered role {label}",
            "permissions": [{"actions": definition["actions"], "notActions": [], "dataActions": definition["data_actions"], "notDataActions": []}],
            "assignableScopes": [place["rg"]],
        }})
        if path not in self.role_definitions:
            self.role_definitions.append(path)
        return f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{role_id}"

    def assign(self, scope, role_definition_id, principal, condition=None):
        path = f"{scope}/providers/Microsoft.Authorization/roleAssignments/{uuid.uuid4()}"
        properties = {"roleDefinitionId": role_definition_id, "principalId": principal, "principalType": "ServicePrincipal"}
        if condition:
            properties["condition"] = condition
            properties["conditionVersion"] = "2.0"
        for attempt in range(12):  # A new identity takes a while to reach the directory.
            status, body = self.call("PUT", path, {"properties": properties})
            if status < 300:
                return
            if body.get("error", {}).get("code") != "PrincipalNotFound" or attempt == 11:
                raise SystemExit(f"assign {role_definition_id} to {principal}: {body}")
            time.sleep(10)

    def grant_rendered(self, place, definitions, assignments, holder, identity, scope_for):
        """Give `identity` every role the module assigns to `holder`; return the actions held."""
        held = {"actions": [], "data_actions": []}
        done = set()
        if holder == MANAGEMENT:
            # Setup defines these; the emitter of the resource they're for assigns them (compute
            # clusters render through an extension, absent here), so management holds them too.
            assignments = assignments + [
                {"label": label, "principal": holder, "definition": label, "predefined": None, "condition": None}
                for label in definitions if label.startswith("setup_management_")
            ]
        for assignment in assignments:
            if assignment["principal"] != holder:
                continue
            key = (assignment["definition"], assignment["predefined"], scope_for(assignment))
            if key in done:
                continue
            done.add(key)
            if assignment["definition"]:
                definition = definitions[assignment["definition"]]
                role = self.role(place, assignment["definition"], definition)
            elif assignment["predefined"] == READER:
                definition = {"actions": ["*/read"], "data_actions": []}
                role = f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{READER}"
            elif assignment["predefined"] == SECRETS_OFFICER:
                definition = {"actions": ["Microsoft.KeyVault/vaults/secrets/*"], "data_actions": ["Microsoft.KeyVault/vaults/secrets/*"]}
                role = f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{SECRETS_OFFICER}"
            else:
                raise SystemExit(f"{assignment['label']}: a role this check doesn't know: {assignment}")
            condition = assignment["condition"]
            if condition:
                condition = condition.replace(f"${{{holder}}}", identity["principal"])
                for key, label in self.runtime_roles.items():
                    created = self.role(place, label, definitions[label])
                    condition = condition.replace(f"${{local.management_runtime_role_ids.{key}}}", created.rsplit("/", 1)[1])
                # Other identities the module creates (a workload's): none exist here.
                condition = re.sub(r"\$\{azurerm_user_assigned_identity\.[a-z0-9_]+\.principal_id\}", "00000000-0000-0000-0000-000000000001", condition)
                if "${" in condition:
                    raise SystemExit(f"unresolved reference in {assignment['label']}: {condition}")
            self.assign(scope_for(assignment), role, identity["principal"], condition)
            if condition:
                # Held only for what the condition lets through; the cases name the role they assign.
                held.setdefault("conditional", []).append({"actions": definition["actions"], "condition": condition})
            else:
                held["actions"] += definition["actions"]
            held["data_actions"] += definition["data_actions"]
        return held

    # Attempts

    def probe(self, place, name, identity, cases):
        """Run `cases` in a container instance as `identity`; return {case: code}."""
        group = f"{place['rg']}/providers/Microsoft.ContainerInstance/containerGroups/{place['group']}-{name}"
        script = base64.b64encode(PROBE.encode()).decode()
        self.must("PUT", group, {
            "location": self.location, "tags": self.tags,
            "identity": {"type": "UserAssigned", "userAssignedIdentities": {identity["id"]: {}}},
            "properties": {
                "osType": "Linux", "restartPolicy": "Never",
                "containers": [{"name": "probe", "properties": {
                    "image": IMAGE,
                    "command": ["bash", "-c", "echo $PROBE | base64 -d > /tmp/probe.py && python3 /tmp/probe.py"],
                    "environmentVariables": [
                        {"name": "PROBE", "value": script},
                        {"name": "CLIENT_ID", "value": identity["client"]},
                        {"name": "CASES", "value": json.dumps(cases)},
                    ],
                    "resources": {"requests": {"cpu": 1, "memoryInGB": 1.5}},
                }}],
            },
        }, api="2023-05-01")
        state = self.wait(group, "2023-05-01", lambda s, b: (b.get("properties", {}).get("instanceView") or {}).get("state") in ("Succeeded", "Failed", "Stopped") or b.get("properties", {}).get("provisioningState") == "Failed", f"probe {name}")
        (self.evidence / f"{place['group']}-{name}-group.json").write_text(json.dumps(state, indent=2))
        if state.get("properties", {}).get("provisioningState") == "Failed":
            raise SystemExit(f"probe {name}: container group failed to provision: {json.dumps(state.get('properties', {}).get('instanceView'))}")
        logs = self.must("GET", f"{group}/containers/probe/logs", api="2023-05-01").get("content", "")
        (self.evidence / f"{place['group']}-{name}.log").write_text(logs)
        results = {}
        for line in logs.splitlines():
            if line.startswith("RESULT "):
                result = json.loads(line[len("RESULT "):])
                results[result["case"]] = result
        if "DONE" not in logs:
            raise SystemExit(f"probe {name} did not finish: {logs[-2000:]}")
        return results

    def management_cases(self, place, principal):
        vault_role = f"{place['vault']}/providers/Microsoft.Authorization/roleAssignments/{uuid.uuid4()}?api-version=2022-04-01"
        custom = place["scale_set_body"]
        return [
            {"name": "tags-extension", "action": "Microsoft.Resources/tags/write", "method": "PATCH", "url": f"{ARM}{place['rg']}/providers/Microsoft.Resources/tags/default?api-version=2021-04-01", "body": {"operation": "Merge", "properties": {"tags": {"signing-fingerprint": "{}"}}}},
            {"name": "group-write", "action": "Microsoft.Resources/subscriptions/resourceGroups/write", "method": "PATCH", "url": f"{ARM}{place['rg']}?api-version=2021-04-01", "body": {"tags": {**self.tags, "probe": "management"}}},
            {"name": "scale-set-custom-data", "action": "Microsoft.Compute/virtualMachineScaleSets/write", "method": "PATCH", "url": f"{ARM}{place['vmss']}?api-version=2024-07-01", "body": {"properties": {"virtualMachineProfile": {"osProfile": {"customData": base64.b64encode(b"unsigned").decode()}}}}},
            {"name": "scale-set-capacity", "action": "Microsoft.Compute/virtualMachineScaleSets/write", "method": "PATCH", "url": f"{ARM}{place['vmss']}?api-version=2024-07-01", "body": {"sku": {"capacity": 0}}},
            {"name": "new-scale-set", "action": "Microsoft.Compute/virtualMachineScaleSets/write", "method": "PUT", "url": f"{ARM}{place['rg']}/providers/Microsoft.Compute/virtualMachineScaleSets/{place['group']}-rogue?api-version=2024-07-01", "body": custom},
            {"name": "vault-secret-write", "data_action": "Microsoft.KeyVault/vaults/secrets/setSecret/action", "resource": "https://vault.azure.net", "method": "PUT", "url": f"{place['vault_uri']}/secrets/signing-state?api-version=7.4", "body": {"value": "{}"}},
            {"name": "vault-role-assignment", "action": "Microsoft.Authorization/roleAssignments/write", "assigns": SECRETS_OFFICER, "method": "PUT", "url": f"{ARM}{vault_role}", "body": {"properties": {"roleDefinitionId": f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{SECRETS_OFFICER}", "principalId": principal, "principalType": "ServicePrincipal"}}},
            {"name": "vault-write", "action": "Microsoft.KeyVault/vaults/write", "method": "PATCH", "url": f"{ARM}{place['vault']}?api-version=2023-07-01", "body": {"properties": {"enableRbacAuthorization": False}}},
        ]

    def machine_cases(self, place):
        return [
            {"name": "read-settings-tag", "action": "Microsoft.Resources/subscriptions/resourceGroups/read", "method": "GET", "url": f"{ARM}{place['rg']}?api-version=2021-04-01"},
            {"name": "list-vaults", "action": "Microsoft.KeyVault/vaults/read", "method": "GET", "url": f"{ARM}{place['rg']}/providers/Microsoft.KeyVault/vaults?api-version=2023-07-01"},
            {"name": "list-identities", "action": "Microsoft.ManagedIdentity/userAssignedIdentities/read", "method": "GET", "url": f"{ARM}{place['rg']}/providers/Microsoft.ManagedIdentity/userAssignedIdentities?api-version=2023-01-31"},
            # The ledger setup wrote stands in as this write; the read then finds it.
            {"name": "write-ledger", "data_action": "Microsoft.KeyVault/vaults/secrets/setSecret/action", "resource": "https://vault.azure.net", "method": "PUT", "url": f"{place['vault_uri']}/secrets/signing-state?api-version=7.4", "body": {"value": "{\"prefix\":\"x\"}"}},
            {"name": "read-ledger", "data_action": "Microsoft.KeyVault/vaults/secrets/getSecret/action", "resource": "https://vault.azure.net", "method": "GET", "url": f"{place['vault_uri']}/secrets/signing-state?api-version=7.4"},
        ]

    def judge(self, variant, cases, held, results):
        for case in cases:
            if "data_action" in case:
                allowed = holds(held["data_actions"], case["data_action"])
            else:
                allowed = holds(held["actions"], case["action"]) or any(
                    holds(grant["actions"], case["action"]) and case.get("assigns", "-") in grant["condition"]
                    for grant in held.get("conditional", [])
                )
            result = results.get(case["name"], {"code": "missing"})
            self.expect(variant, case["name"], "Success" if allowed else "Denied", denied(result["code"]), {"held": allowed, "result": result})

    def variant(self, scratch, variant, protected):
        module = self.render(scratch, protected)
        definitions, assignments, runtime_roles = parse_roles(module)
        self.runtime_roles = runtime_roles
        self.record(f"{variant}-rendered-roles", {"definitions": definitions, "assignments": assignments, "runtimeRoles": runtime_roles})
        place = self.build_group(variant, protected)
        place["scale_set_body"] = self.scale_set_body(place["net"])
        management = self.identity(place, "management")
        everywhere = lambda assignment: place["rg"]
        held = self.grant_rendered(place, definitions, assignments, MANAGEMENT, management, everywhere)
        self.record(f"{variant}-management-held", held)
        probes = [("management", management, self.management_cases(place, management["principal"]), held)]
        if protected:
            verifier = self.identity(place, "verifier")
            vheld = self.grant_rendered(place, definitions, assignments, VERIFIER, verifier, lambda a: place["vault"] if a["predefined"] == SECRETS_OFFICER else place["rg"])
            probes.append(("verifier", verifier, [c for c in self.management_cases(place, verifier["principal"]) if c["name"] == "scale-set-capacity"], vheld))
            machine = self.identity(place, "machine")
            # What azure_machine_signing_grants renders for the machine identity.
            if "management_machine_signing" not in definitions:
                raise SystemExit("the protected module has no machine-signing role")
            role = self.role(place, "management_machine_signing", definitions["management_machine_signing"])
            self.assign(place["rg"], role, machine["principal"])
            self.assign(place["vault"], f"/subscriptions/{self.subscription}/providers/Microsoft.Authorization/roleDefinitions/{SECRETS_OFFICER}", machine["principal"])
            mheld = {"actions": definitions["management_machine_signing"]["actions"] + ["Microsoft.KeyVault/vaults/secrets/*"], "data_actions": ["Microsoft.KeyVault/vaults/secrets/*"]}
            probes.append(("machine", machine, self.machine_cases(place), mheld))
            bare = self.identity(place, "bare-machine")
            probes.append(("machine-without-grants", bare, self.machine_cases(place), {"actions": [], "data_actions": []}))
        time.sleep(120)  # Role assignments reach authorization after a while.
        for name, identity, cases, granted in probes:
            results = self.probe(place, name, identity, cases)
            self.judge(f"{variant}-{name}", cases, granted, results)

    # Cleanup and audit

    def cleanup(self):
        failures = []
        for rg in self.groups:
            status, body = self.call("DELETE", rg, api="2021-04-01")
            if status >= 300 and status != 404:
                failures.append(f"delete {rg}: {body}")
        for rg in self.groups:
            try:
                self.wait(rg, "2021-04-01", lambda s, b: s == 404, f"delete {rg}")
            except SystemExit as error:
                failures.append(str(error))
        for path in self.role_definitions:
            status, body = self.call("DELETE", path)
            if status >= 300 and status != 404:
                failures.append(f"delete {path}: {body}")
        for name in self.vaults:
            path = f"/subscriptions/{self.subscription}/providers/Microsoft.KeyVault/locations/{self.location}/deletedVaults/{name}"
            status, _ = self.call("GET", path, api="2023-07-01")
            if status == 200:
                self.call("POST", f"{path}/purge", api="2023-07-01")
        return failures

    def audit(self):
        sub = f"/subscriptions/{self.subscription}"
        _, groups = self.call("GET", f"{sub}/resourcegroups", api="2021-04-01")
        left = [g["name"] for g in groups.get("value", []) if g["name"].startswith(self.args.prefix)]
        _, roles = self.call("GET", f"{sub}/providers/Microsoft.Authorization/roleDefinitions?$filter=type%20eq%20'CustomRole'")
        left += [r["properties"]["roleName"] for r in roles.get("value", []) if r["properties"]["roleName"].startswith(self.args.prefix)]
        self.record("cleanup-audit", {"left": left})
        return left


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--renderer", required=True)
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--location", default="eastus")
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z][a-z0-9-]{2,14}", args.prefix):
        raise SystemExit("--prefix: 3 to 15 lowercase letters, digits and hyphens, starting with a letter")

    run = Run(args)
    failures = []
    try:
        with tempfile.TemporaryDirectory() as scratch:
            run.variant(scratch, "unprotected", False)
            run.variant(scratch, "protected", True)
    finally:
        failures += [f"cleanup: {failure}" for failure in run.cleanup()]
        time.sleep(30)
        failures += [f"left behind: {item}" for item in run.audit()]

    failures += [f"{r['variant']} {r['case']}: expected {r['expected']}, got {r['actual']}" for r in run.results if not r["ok"]]
    # A protected management identity holding any of these is a broken lock, whatever Azure did.
    for r in run.results:
        if r["variant"] == "protected-management" and r["expected"] == "Success":
            failures.append(f"protected management holds {r['case']}")
    run.record("summary", {"results": run.results, "failures": failures})
    if failures:
        print("\n".join(failures), file=sys.stderr)
        sys.exit(1)
    print(f"all {len(run.results)} checks passed; nothing left behind")


if __name__ == "__main__":
    main()
