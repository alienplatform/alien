#!/usr/bin/env python3
"""Check on real GCP that a protected setup keeps from the management identity what only the
verifier may do.

Renders `app/` (an automatic setup, so the management identity holds every set it can) as a
GCP Terraform module twice with the `render_setup` example of alien-cli-common: protected (with
signing) and unprotected. From each module it
takes the project-level roles bound to the management service account, exactly as rendered
(permissions, predefined roles and IAM conditions), and grants them to a test service account.
As that account it then tries what a compromised control plane would try:

  * update a Cloud Run service the verifier owns (validate only);
  * set an instance's metadata (on an instance that does not exist);
  * act as the node service account (IAM's own answer for that one permission);
  * create an instance template that runs as the node service account;
  * create an instance that runs as the node service account (from an image that does not
    exist, so no machine can start);
  * replace the startup metadata of a managed instance group's machines;
  * point the group at another instance template;
  * resize the group (scaling goes through the verifier on protected installs);
  * write the project's IAM policy (the policy it already has, so nothing changes);
  * write a signed input secret's IAM policy and add a version to it;
  * add a version to the signing settings parameter in Parameter Manager.

The protected account must be refused each attempt. The unprotected account must pass the
permission check (succeed, or fail for a reason other than permission), so the check cannot
pass on attempts GCP refuses for an unrelated reason. The signing settings parameter is refused
to both (management never holds Parameter Manager writes), and so is a signed input secret
(no grant reaches `signed-` names; protected bindings also exclude them by name). Both must still read the Cloud Run service.
Compute Engine names every permission a refused request lacks, so for instance creation and
group metadata the check compares those lists: the protected account must lack creating and
reconfiguring machines, the unprotected one must hold both.

Everything created is named with --prefix and labeled with --label where GCP takes labels;
custom role IDs, which allow no '-', use the prefix with '_'. All of it is deleted before
exit, and the run then lists what still carries the prefix and fails if anything does. Custom
roles stay soft-deleted for a while, which GCP does not let a run avoid.

No machine starts: the instance group has no instances and the instance cannot be created. A
run costs a few cents at most.

    python3 scripts/gcp-setup-locks/check.py --renderer target/debug/examples/render_setup \\
        --project my-test-project --region us-central1 --token-file /path/to/token \\
        --prefix locks-check --label purpose=locks-check --evidence /tmp/gcp-locks

The token file holds an access token of a project owner. It is read for every call and never
printed.
"""

import argparse
import base64
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

HERE = pathlib.Path(__file__).resolve().parent
IMAGE = "us-docker.pkg.dev/cloudrun/container/hello"
MACHINE_IMAGE = "projects/debian-cloud/global/images/family/debian-12"


def log(message):
    print(f"[{time.strftime('%H:%M:%S')}] {message}", flush=True)


class HttpError(Exception):
    def __init__(self, status, body):
        super().__init__(f"HTTP {status}: {body[:600]}")
        self.status = status
        self.body = body


def outcome(error):
    """`allowed` for success, `denied` for a permission error, else `other:<status>`."""
    if error is None:
        return "allowed"
    if error.status == 403 or "PERMISSION_DENIED" in error.body:
        return "denied"
    return f"other:{error.status}"


def missing(error):
    """The permissions a Compute Engine refusal names: it lists every one the caller lacks."""
    return set() if error is None else set(re.findall(r"Required '([a-zA-Z.]+)' permission", error.body))


# --- reading the rendered module ----------------------------------------------------------------


def blocks(text, kind):
    """`(label, body)` of each `resource "<kind>" "<label>" { ... }` block."""
    found = []
    for match in re.finditer(r'^resource "%s" "([^"]+)" \{\n' % re.escape(kind), text, re.M):
        end = text.index("\n}\n", match.end())
        found.append((match.group(1), text[match.end():end]))
    return found


def hcl_string(value):
    return json.loads(value)


def management_grants(module_dir):
    """The project-level grants of the management service account: custom roles as permission
    sets with their binding's condition, and predefined roles."""
    text = (module_dir / "management.tf").read_text()
    roles = {}
    for label, body in blocks(text, "google_project_iam_custom_role"):
        listed = re.search(r"permissions\s+=\s+\[\n(.*?)\n  \]", body, re.S)
        roles[label] = [hcl_string(line.strip().rstrip(",")) for line in listed.group(1).splitlines()]
    custom, predefined = [], set()
    for _, body in blocks(text, "google_project_iam_member"):
        # Protected modules are not run through `terraform fmt`, so spacing differs.
        if not re.search(r'member\s+=\s+"serviceAccount:\$\{google_service_account\.management\.email\}"', body):
            continue
        role = re.search(r"^  role +=(.*)$", body, re.M).group(1).strip()
        condition = re.search(r'expression += (".*")$', body, re.M)
        expression = hcl_string(condition.group(1)) if condition else None
        reference = re.search(r"google_project_iam_custom_role\.([A-Za-z0-9_]+)\[0\]", role)
        if reference:
            custom.append((roles[reference.group(1)], expression))
        else:
            predefined.add(hcl_string(role))
    return custom, sorted(predefined)


# --- the run ------------------------------------------------------------------------------------


class Run:
    def __init__(self, args):
        self.args = args
        self.project, self.region, self.zone = args.project, args.region, f"{args.region}-a"
        self.suffix = uuid.uuid4().hex[:4]
        self.name = f"{args.prefix}-{self.suffix}"
        self.role_prefix = self.name.replace("-", "_")
        key, value = args.label.split("=", 1)
        self.labels = {key: value}
        self.evidence = pathlib.Path(args.evidence)
        self.evidence.mkdir(parents=True, exist_ok=True)
        self.work = pathlib.Path(tempfile.mkdtemp(prefix="gcp-setup-locks-"))
        self.cleanups = []
        self.results = []
        self.number = None
        self.node = f"{self.name}-node@{self.project}.iam.gserviceaccount.com"
        self.service = f"{self.name}-fn-api"
        self.input_secret = f"signed-{self.name}-input-api-key"
        self.settings_parameter = f"signed-{self.name}-fingerprint"
        self.mig = f"{self.name}-mig"
        self.template = f"{self.name}-tpl"

    # HTTP

    def admin_token(self):
        return pathlib.Path(self.args.token_file).read_text().strip()

    def call(self, method, url, body=None, token=None, ok_statuses=(200,)):
        request = urllib.request.Request(url, method=method, data=None if body is None else json.dumps(body).encode())
        request.add_header("Authorization", f"Bearer {token or self.admin_token()}")
        request.add_header("Content-Type", "application/json")
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                text = response.read().decode()
                return json.loads(text) if text else {}
        except urllib.error.HTTPError as error:
            raise HttpError(error.code, error.read().decode()) from None

    def attempt(self, method, url, body=None, token=None):
        try:
            return self.call(method, url, body, token), None
        except HttpError as error:
            return None, error

    def wait_operation(self, operation, token=None):
        """Wait for a Compute Engine operation."""
        url = operation.get("selfLink")
        for _ in range(120):
            if operation.get("status") == "DONE":
                if operation.get("error"):
                    raise RuntimeError(f"operation failed: {operation['error']}")
                return operation
            time.sleep(2)
            operation = self.call("GET", url, token=token)
        raise RuntimeError("operation timed out")

    def wait_lro(self, operation, base):
        for _ in range(150):
            if operation.get("done"):
                if operation.get("error"):
                    raise RuntimeError(f"operation failed: {operation['error']}")
                return operation
            time.sleep(2)
            operation = self.call("GET", f"{base}/{operation['name']}")
        raise RuntimeError("operation timed out")

    def compute(self, path):
        return f"https://compute.googleapis.com/compute/v1/projects/{self.project}/{path}"

    # Evidence

    def record(self, name, payload):
        (self.evidence / f"{name}.json").write_text(json.dumps(payload, indent=2, default=str) + "\n")

    def expect(self, variant, case, expected, error, detail=None):
        actual = outcome(error)
        ok = actual == expected
        self.results.append({"variant": variant, "case": case, "expected": expected, "actual": actual, "ok": ok})
        self.record(f"{variant}-{case}", {"expected": expected, "actual": actual,
                                          "error": None if error is None else error.body[:2000], "detail": detail})
        log(f"{'ok  ' if ok else 'FAIL'} {variant:11} {case:34} expected {expected:24} got {actual}")

    def expect_missing(self, variant, case, permissions, error):
        """Compute Engine checks every permission a request needs and names each one the caller
        lacks. A protected caller must lack all of `permissions`; an unprotected one none."""
        lacked = missing(error)
        if variant == "protected":
            ok, expected = permissions <= lacked, f"lacks {sorted(permissions)}"
        else:
            ok, expected = not permissions & lacked, f"holds {sorted(permissions)}"
        self.results.append({"variant": variant, "case": case, "expected": expected, "actual": sorted(lacked), "ok": ok})
        self.record(f"{variant}-{case}", {"expected": expected, "lacked": sorted(lacked),
                                          "error": None if error is None else error.body[:2000]})
        log(f"{'ok  ' if ok else 'FAIL'} {variant:11} {case:34} {expected}; lacked {sorted(lacked)}")

    # Rendering

    def render(self):
        modules = {}
        signing = {"fingerprint": "SHA256:" + "0123456789abcdef" * 4, "channel": "production"}
        for variant, settings in [("protected", {"setupManagement": "automatic", "signing": signing}),
                                  ("unprotected", {"setupManagement": "automatic"})]:
            path = self.work / f"{variant}.json"
            path.write_text(json.dumps(settings))
            rendered = self.work / f"{variant}-module.json"
            subprocess.run([str(self.args.renderer), "terraform-gcp", str(HERE / "app" / "alien.json"),
                            str(path), str(rendered)], check=True, capture_output=True)
            output = self.work / variant
            output.mkdir()
            for name, contents in json.loads(rendered.read_text()).items():
                (output / name).write_text(contents)
            modules[variant] = output
        self.record("rendered-signing", {"signing.tf": (modules["protected"] / "signing.tf").read_text()})
        return modules

    def substitute(self, expression):
        return (expression.replace("${data.google_project.current.number}", self.number)
                .replace("${local.resource_prefix}", self.name)
                .replace("${var.gcp_project}", self.project))

    # Setup

    def caller(self):
        info = urllib.request.urlopen(urllib.request.Request(
            "https://oauth2.googleapis.com/tokeninfo", data=urllib.parse.urlencode({"access_token": self.admin_token()}).encode()),
            timeout=30)
        email = json.loads(info.read().decode())["email"]
        return f"serviceAccount:{email}" if email.endswith(".gserviceaccount.com") else f"user:{email}"

    def service_account(self, account_id, description):
        self.call("POST", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts", {
            "accountId": account_id,
            "serviceAccount": {"displayName": f"{account_id} ({self.args.label})", "description": description},
        })
        email = f"{account_id}@{self.project}.iam.gserviceaccount.com"
        self.cleanups.append(("service account", lambda: self.call(
            "DELETE", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{email}")))
        for _ in range(30):
            try:
                self.call("GET", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{email}")
                return email
            except HttpError:
                time.sleep(2)
        raise RuntimeError(f"{email} did not appear")

    def edit_project_policy(self, edit):
        base = f"https://cloudresourcemanager.googleapis.com/v1/projects/{self.project}"
        for _ in range(5):
            policy = self.call("POST", f"{base}:getIamPolicy", {"options": {"requestedPolicyVersion": 3}})
            edit(policy)
            policy["version"] = 3
            try:
                return self.call("POST", f"{base}:setIamPolicy", {"policy": policy})
            except HttpError as error:
                if error.status != 409:
                    raise
                time.sleep(3)
        raise RuntimeError("project policy kept changing")

    def remove_members(self, members):
        def edit(policy):
            for binding in policy.get("bindings", []):
                binding["members"] = [m for m in binding["members"] if m not in members]
            policy["bindings"] = [b for b in policy.get("bindings", []) if b["members"]]
        self.edit_project_policy(edit)

    def custom_role(self, role_id, permissions):
        base = f"https://iam.googleapis.com/v1/projects/{self.project}/roles"
        dropped = []
        while True:
            try:
                role = self.call("POST", base, {"roleId": role_id, "role": {
                    "title": f"{self.name} {role_id}", "description": f"Locks check ({self.args.label}).",
                    "includedPermissions": sorted(set(permissions) - set(dropped)), "stage": "GA"}})
                break
            except HttpError as error:
                unsupported = re.findall(r"Permission ([a-zA-Z0-9.]+) is not (?:valid|supported)", error.body)
                if error.status != 400 or not unsupported:
                    raise
                dropped.extend(unsupported)
        self.cleanups.append(("custom role", lambda: self.call("DELETE", f"{base}/{role_id}")))
        return role["name"], dropped

    def grant(self, variant, email, module):
        custom, predefined = management_grants(module)
        member = f"serviceAccount:{email}"
        groups = {}
        for permissions, expression in custom:
            groups.setdefault(expression, set()).update(permissions)
        bindings, dropped = [], {}
        for index, (expression, permissions) in enumerate(sorted(groups.items(), key=lambda item: item[0] or "")):
            name, unsupported = self.custom_role(f"{self.role_prefix}_{variant[:4]}_{index}", permissions)
            if unsupported:
                dropped[name] = unsupported
            binding = {"role": name, "members": [member]}
            if expression:
                binding["condition"] = {"title": f"c{index}", "expression": self.substitute(expression)}
            bindings.append(binding)
        bindings.extend({"role": role, "members": [member]} for role in predefined
                        if role != "roles/iam.workloadIdentityUser")
        self.record(f"{variant}-grants", {"bindings": bindings, "unsupportedInCustomRoles": dropped,
                                          "permissions": sorted({p for _, ps in groups.items() for p in ps})})
        self.edit_project_policy(lambda policy: policy.setdefault("bindings", []).extend(bindings))
        return sorted({p for ps in groups.values() for p in ps})

    def setup(self):
        log(f"creating the node account, the verifier-owned service, the signing values and a group of no machines ({self.name})")
        self.service_account(f"{self.name}-node", "Stands in for the node service account.")
        # The Cloud Run service the verifier owns, created as setup would.
        run_base = f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services"
        operation = self.call("POST", f"{run_base}?serviceId={self.service}", {
            "labels": self.labels, "ingress": "INGRESS_TRAFFIC_INTERNAL_ONLY",
            "template": {"serviceAccount": self.node, "containers": [{"image": IMAGE}],
                         "scaling": {"minInstanceCount": 0, "maxInstanceCount": 1}},
        })
        self.cleanups.append(("cloud run service", lambda: self.call("DELETE", f"{run_base}/{self.service}")))
        self.wait_lro(operation, "https://run.googleapis.com/v2")
        # A signed input stored as a secret, and the settings parameter, as protected setup
        # writes them.
        secrets = f"https://secretmanager.googleapis.com/v1/projects/{self.project}/secrets"
        self.call("POST", f"{secrets}?secretId={self.input_secret}", {"labels": self.labels, "replication": {"automatic": {}}})
        self.cleanups.append(("input secret", lambda: self.call("DELETE", f"{secrets}/{self.input_secret}")))
        self.call("POST", f"{secrets}/{self.input_secret}:addVersion", {"payload": {"data": base64.b64encode(b"vendor value").decode()}})
        parameters = f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/parameters"
        self.call("POST", f"{parameters}?parameterId={self.settings_parameter}", {"labels": self.labels, "format": "JSON"})
        self.cleanups.append(("settings parameter", lambda: self.call("DELETE", f"{parameters}/{self.settings_parameter}")))
        self.call("POST", f"{parameters}/{self.settings_parameter}/versions?parameterVersionId=v1",
                  {"payload": {"data": base64.b64encode(json.dumps({"fingerprint": "SHA256:" + "0123456789abcdef" * 4, "prefix": self.name, "channel": "production"}).encode()).decode()}})
        self.cleanups.append(("settings version", lambda: self.call("DELETE", f"{parameters}/{self.settings_parameter}/versions/v1")))
        # An instance template running as the node account and a group of zero machines from it.
        for template in [self.template, f"{self.template}-other"]:
            self.wait_operation(self.call("POST", self.compute("global/instanceTemplates"), self.template_body(template)))
            self.cleanups.append(("instance template", lambda template=template: self.wait_operation(
                self.call("DELETE", self.compute(f"global/instanceTemplates/{template}")))))
        self.wait_operation(self.call("POST", self.compute(f"zones/{self.zone}/instanceGroupManagers"), {
            "name": self.mig, "baseInstanceName": self.mig, "targetSize": 0,
            "instanceTemplate": f"projects/{self.project}/global/instanceTemplates/{self.template}",
        }))
        self.cleanups.append(("instance group", lambda: self.wait_operation(
            self.call("DELETE", self.compute(f"zones/{self.zone}/instanceGroupManagers/{self.mig}")))))

    def template_body(self, name):
        return {"name": name, "properties": {
            "machineType": "e2-micro", "labels": self.labels,
            "disks": [{"boot": True, "autoDelete": True, "initializeParams": {"sourceImage": MACHINE_IMAGE}}],
            "networkInterfaces": [{"network": "global/networks/default"}],
            "serviceAccounts": [{"email": self.node, "scopes": ["https://www.googleapis.com/auth/cloud-platform"]}],
        }}

    def impersonate(self, email):
        response = self.call("POST", f"https://iamcredentials.googleapis.com/v1/projects/-/serviceAccounts/{email}:generateAccessToken",
                             {"scope": ["https://www.googleapis.com/auth/cloud-platform"], "lifetime": "1800s"})
        return response["accessToken"]

    # Attempts

    def run_service_update(self, token):
        url = (f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services/"
               f"{self.service}?validateOnly=true")
        return self.attempt("PATCH", url, {"labels": self.labels, "ingress": "INGRESS_TRAFFIC_INTERNAL_ONLY", "template": {
            "serviceAccount": self.node, "containers": [{"image": IMAGE, "env": [{"name": "INJECTED", "value": "1"}]}]}}, token)[1]

    def attempts(self, variant, token):
        run_url = f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services/{self.service}"
        self.expect(variant, "read-verifier-service", "allowed", self.attempt("GET", run_url, token=token)[1])
        self.expect(variant, "update-verifier-service", "denied" if variant == "protected" else "allowed",
                    self.run_service_update(token))
        # The instance does not exist: a caller that may set metadata gets "not found".
        _, error = self.attempt("POST", self.compute(f"zones/{self.zone}/instances/{self.name}-none/setMetadata"),
                                {"items": [{"key": "startup-script", "value": "true"}]}, token)
        if variant == "unprotected":
            self.expect(variant, "set-instance-metadata", "other:404", error)
        else:
            self.expect_missing(variant, "set-instance-metadata", {"compute.instances.setMetadata"}, error)
        # Acting as the node account is what lets a template or an instance run as it. IAM
        # answers for that permission alone.
        tested = self.call("POST", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{self.node}:testIamPermissions",
                           {"permissions": ["iam.serviceAccounts.actAs"]}, token).get("permissions", [])
        self.expect(variant, "act-as-node", "denied" if variant == "protected" else "allowed",
                    None if tested else HttpError(403, "PERMISSION_DENIED: testIamPermissions returned nothing"))
        template = f"{self.name}-tpl-{variant[:4]}"
        created, error = self.attempt("POST", self.compute("global/instanceTemplates"), self.template_body(template), token)
        if created is not None:
            self.cleanups.append(("instance template", lambda: self.wait_operation(
                self.call("DELETE", self.compute(f"global/instanceTemplates/{template}")))))
            self.wait_operation(created, token)
        self.expect(variant, "create-template-as-node", "denied" if variant == "protected" else "allowed", error)
        instance = self.template_body(f"{self.name}-vm-{variant[:4]}")
        instance["properties"]["disks"][0]["initializeParams"]["sourceImage"] = f"projects/{self.project}/global/images/{self.name}-none"
        body = {"name": instance["name"], **instance["properties"], "machineType": f"zones/{self.zone}/machineTypes/e2-micro"}
        # The image does not exist, so no machine can start whatever the caller holds.
        self.expect_missing(variant, "create-instance-as-node", {"compute.instances.create"},
                            self.attempt("POST", self.compute(f"zones/{self.zone}/instances"), body, token)[1])
        base = f"https://cloudresourcemanager.googleapis.com/v1/projects/{self.project}"
        policy = self.call("POST", f"{base}:getIamPolicy", {"options": {"requestedPolicyVersion": 3}})
        policy["version"] = 3
        self.expect(variant, "set-project-iam-policy", "denied" if variant == "protected" else "allowed",
                    self.attempt("POST", f"{base}:setIamPolicy", {"policy": policy}, token)[1])
        secret = f"https://secretmanager.googleapis.com/v1/projects/{self.project}/secrets/{self.input_secret}"
        secret_policy = self.call("GET", f"{secret}:getIamPolicy")
        self.expect(variant, "set-input-secret-iam-policy", "denied",
                    self.attempt("POST", f"{secret}:setIamPolicy", {"policy": secret_policy}, token)[1])
        added, error = self.attempt("POST", f"{secret}:addVersion", {"payload": {"data": base64.b64encode(b"cp value").decode()}}, token)
        # No management grant reaches `signed-` names; protected bindings also exclude them.
        self.expect(variant, "write-input-secret", "denied", error)
        parameter = (f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/"
                     f"parameters/{self.settings_parameter}")
        version = f"cp{variant[:4]}"
        _, error = self.attempt("POST", f"{parameter}/versions?parameterVersionId={version}", {"payload": {
            "data": base64.b64encode(b'{"fingerprint":"SHA256:00"}').decode()}}, token)
        if error is None:
            self.cleanups.append(("settings version", lambda: self.call("DELETE", f"{parameter}/versions/{version}")))
        self.expect(variant, "write-signing-settings", "denied", error)
        mig = self.compute(f"zones/{self.zone}/instanceGroupManagers/{self.mig}")
        resized, error = self.attempt("POST", f"{mig}/resize?size=0", None, token)
        if resized is not None:
            self.wait_operation(resized, token)
        self.expect(variant, "resize-group", "denied" if variant == "protected" else "allowed", error)
        other = f"projects/{self.project}/global/instanceTemplates/{self.template}-other"
        pointed, error = self.attempt("POST", f"{mig}/setInstanceTemplate", {"instanceTemplate": other}, token)
        if pointed is not None:
            self.wait_operation(pointed, token)
            self.wait_operation(self.call("POST", f"{mig}/setInstanceTemplate", {
                "instanceTemplate": f"projects/{self.project}/global/instanceTemplates/{self.template}"}))
        # Compute Engine names what the caller lacks; unprotected management lacks only labels here.
        self.expect_missing(variant, "point-group-at-another-template", {"compute.instanceGroupManagers.update"}, error)
        patched, error = self.attempt("PATCH", mig, {"allInstancesConfig": {"properties": {
            "metadata": {"startup-script": "true"}}}}, token)
        if patched is not None:
            self.wait_operation(patched, token)
        # Replacing the metadata of a group's machines is checked as creating them with it.
        self.expect_missing(variant, "replace-group-metadata", {"compute.instances.create", "compute.instances.setMetadata"}, error)
        if patched is not None:
            # Leave the group as setup made it.
            self.wait_operation(self.call("PATCH", mig, {"allInstancesConfig": {"properties": {"metadata": {}}}}))

    def wait_until_effective(self, variant, token):
        """IAM changes take a while to apply; wait until a kept permission works."""
        run_url = f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services/{self.service}"
        for _ in range(60):
            _, error = self.attempt("GET", run_url, token=token)
            if error is None and (variant == "protected" or self.run_service_update(token) is None):
                time.sleep(30)
                return
            time.sleep(10)
        raise RuntimeError(f"the {variant} grants did not apply")

    # Teardown

    def cleanup(self):
        log("deleting everything the run created")
        members = {f"serviceAccount:{email}" for email in self.accounts}
        try:
            self.remove_members(members)
        except Exception as error:  # noqa: BLE001 - report every cleanup failure
            log(f"cleanup failed: project bindings: {error}")
            self.results.append({"variant": "cleanup", "case": "project bindings", "ok": False})
        for kind, action in reversed(self.cleanups):
            try:
                action()
            except Exception as error:  # noqa: BLE001
                if isinstance(error, HttpError) and error.status == 404:
                    continue
                log(f"cleanup failed: {kind}: {error}")
                self.results.append({"variant": "cleanup", "case": kind, "ok": False})

    def audit(self):
        """Everything that still carries the run's name, by kind."""
        found = {}
        accounts = self.call("GET", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts?pageSize=100")
        found["service accounts"] = [a["email"] for a in accounts.get("accounts", []) if self.args.prefix in a["email"]]
        roles = self.call("GET", f"https://iam.googleapis.com/v1/projects/{self.project}/roles?pageSize=1000")
        found["custom roles (not deleted)"] = [r["name"] for r in roles.get("roles", [])
                                               if self.args.prefix.replace("-", "_") in r["name"] and not r.get("deleted")]
        services = self.call("GET", f"https://run.googleapis.com/v2/projects/{self.project}/locations/{self.region}/services")
        found["cloud run services"] = [s["name"] for s in services.get("services", []) if self.args.prefix in s["name"]]
        secrets = self.call("GET", f"https://secretmanager.googleapis.com/v1/projects/{self.project}/secrets")
        found["secrets"] = [s["name"] for s in secrets.get("secrets", []) if self.args.prefix in s["name"]]
        parameters = self.call("GET", f"https://parametermanager.googleapis.com/v1/projects/{self.project}/locations/global/parameters")
        found["parameters"] = [p["name"] for p in parameters.get("parameters", []) if self.args.prefix in p["name"]]
        for kind, path in [("instance templates", "global/instanceTemplates"),
                           ("instance groups", f"zones/{self.zone}/instanceGroupManagers"),
                           ("instances", f"zones/{self.zone}/instances")]:
            items = self.call("GET", self.compute(path)).get("items", [])
            found[kind] = [item["name"] for item in items if self.args.prefix in item["name"]]
        policy = self.call("POST", f"https://cloudresourcemanager.googleapis.com/v1/projects/{self.project}:getIamPolicy",
                           {"options": {"requestedPolicyVersion": 3}})
        found["project policy members"] = sorted({m for b in policy.get("bindings", []) for m in b["members"]
                                                  if self.args.prefix in m or self.args.prefix.replace("-", "_") in b["role"]})
        self.record("audit", found)
        left = {kind: names for kind, names in found.items() if names}
        log("audit: nothing left" if not left else f"audit: left behind {left}")
        return not left

    def main(self):
        modules = self.render()
        self.number = self.call("GET", f"https://cloudresourcemanager.googleapis.com/v1/projects/{self.project}")["projectNumber"]
        caller = self.caller()
        self.accounts = [self.node]
        try:
            self.setup()
            identities = {}
            for variant in ["protected", "unprotected"]:
                email = self.service_account(f"{self.name}-{variant[:4]}", f"Holds the {variant} management grants.")
                self.accounts.append(email)
                # The caller mints tokens for the account, as the control plane would get them.
                self.call("POST", f"https://iam.googleapis.com/v1/projects/{self.project}/serviceAccounts/{email}:setIamPolicy",
                          {"policy": {"bindings": [{"role": "roles/iam.serviceAccountTokenCreator", "members": [caller]}]}})
                permissions = self.grant(variant, email, modules[variant])
                log(f"{variant}: {len(permissions)} permissions granted to {email}")
                identities[variant] = email
            for variant, email in identities.items():
                for _ in range(30):
                    try:
                        token = self.impersonate(email)
                        break
                    except HttpError:
                        time.sleep(10)
                else:
                    raise RuntimeError(f"cannot act as {email}")
                self.wait_until_effective(variant, token)
                self.attempts(variant, token)
        finally:
            self.cleanup()
            time.sleep(30)  # Deletions reach the listing APIs after a short delay.
            clean = self.audit()
            self.record("results", self.results)
        failed = [r for r in self.results if not r["ok"]]
        log(f"{len(self.results) - len(failed)} of {len(self.results)} checks passed")
        return 0 if clean and not failed else 1


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--renderer", required=True, type=pathlib.Path, help="the render_setup example of alien-cli-common")
    parser.add_argument("--project", required=True)
    parser.add_argument("--region", default="us-central1")
    parser.add_argument("--token-file", required=True)
    parser.add_argument("--prefix", required=True, help="names everything the run creates; at most 12 characters")
    parser.add_argument("--label", required=True, help="key=value label for what takes labels")
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()
    if len(args.prefix) > 12:
        parser.error("--prefix must be at most 12 characters: service account IDs hold 30")
    sys.exit(Run(args).main())


if __name__ == "__main__":
    main()
