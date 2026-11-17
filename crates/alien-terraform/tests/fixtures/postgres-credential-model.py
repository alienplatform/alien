"""Native AWS provider protocol test, not a cloud or SQL authentication test.

Only Secrets Manager and RDS requests used by this fixture are accepted. Values
stay in memory; every saved plan/state is checked without logging credentials.
"""
import http.server
import json
import os
import pathlib
import re
import secrets
import shutil
import subprocess
import sys
import threading
import urllib.parse
import xml.sax.saxutils
import zipfile

cli = os.environ.get("TF_CLI", "terraform")
assert cli in ("terraform", "tofu")
env = {k: v for k, v in os.environ.items() if k in ["PATH", "HOME", "TMPDIR", "TF_CLI_CONFIG_FILE"]}
env |= {"TF_CLI": cli, "AWS_EC2_METADATA_DISABLED": "true"}
passed = False
root = pathlib.Path(sys.argv[1])
legacy_migration = len(sys.argv) > 2 and sys.argv[2] == "legacy-migration"
secret_arn = "arn:aws:secretsmanager:us-east-1:123456789012:secret:rds!cluster-MODEL-abcdef"
cluster_arn = "arn:aws:rds:us-east-1:123456789012:cluster:credential-model-db"
versions = {}
current = None
cluster = None
db_password = None
mutations = []
reads = []
refuse_database = False
unexpected = []
denied_operations = []
managed = not legacy_migration
backup_days = "7"
instance = False
instance_minor_upgrade = None
rotation_enabled = True


def cluster_xml():
    fields = {
        "DBClusterIdentifier": "credential-model-db", "DBClusterArn": cluster_arn,
        "Status": "available", "Engine": "aurora-postgresql", "EngineMode": "provisioned",
        "EngineVersion": "16", "MasterUsername": "app_admin", "DatabaseName": "app",
        "Endpoint": "database.example.invalid", "ReaderEndpoint": "reader.example.invalid",
        "Port": "5432", "StorageEncrypted": "true", "BackupRetentionPeriod": backup_days,
        "DBSubnetGroup": "subnets", "DBClusterParameterGroup": "parameters",
        "PreferredBackupWindow": "07:00-08:00", "PreferredMaintenanceWindow": "sun:09:00-sun:10:00",
        "DbClusterResourceId": "cluster-MODEL", "CopyTagsToSnapshot": "false",
    }
    tags = "<TagList>" + "".join(f"<Tag><Key>{k}</Key><Value>{v}</Value></Tag>" for k, v in [
        ("managed-by", "setup"), ("deployment", "credential-model"), ("resource", "db"), ("resource-type", "postgres")]) + "</TagList>"
    return "<DBCluster>" + "".join(f"<{k}>{xml.sax.saxutils.escape(v)}</{k}>" for k, v in fields.items()) + "<ServerlessV2ScalingConfiguration><MinCapacity>0</MinCapacity><MaxCapacity>2</MaxCapacity></ServerlessV2ScalingConfiguration>" + (f"<MasterUserSecret><SecretArn>{secret_arn}</SecretArn><SecretStatus>active</SecretStatus><KmsKeyId>alias/aws/secretsmanager</KmsKeyId></MasterUserSecret>" if managed else "") + tags + "</DBCluster>"


def instance_xml():
    fields = {"DBInstanceIdentifier":"credential-model-db-1", "DBClusterIdentifier":"credential-model-db",
        "DBInstanceArn":"arn:aws:rds:us-east-1:123456789012:db:credential-model-db-1",
        "DBInstanceStatus":"available", "Engine":"aurora-postgresql", "EngineVersion":"16",
        "DBInstanceClass":"db.serverless", "AutoMinorVersionUpgrade":instance_minor_upgrade, "PubliclyAccessible":"false", "StorageEncrypted":"true",
        "AvailabilityZone":"us-east-1a", "DbiResourceId":"db-MODEL", "DBParameterGroups":""}
    return "<DBInstance>" + "".join(f"<{k}>{v}</{k}>" for k,v in fields.items()) + "<TagList>" + "".join(f"<Tag><Key>{k}</Key><Value>{v}</Value></Tag>" for k,v in [("managed-by","setup"),("deployment","credential-model"),("resource","db"),("resource-type","postgres")]) + "</TagList></DBInstance>"


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        global current, cluster, db_password, backup_days, instance, rotation_enabled, instance_minor_upgrade, managed
        body = self.rfile.read(int(self.headers["Content-Length"]))
        target = self.headers.get("X-Amz-Target", "").split(".")[-1]
        if target:
            data = json.loads(body)
            # Metadata and cancellation are permitted; secret values never are.
            assert data["SecretId"] == secret_arn
            if target == "CancelRotateSecret":
                assert instance, "rotation cancellation must wait for the writer"
                rotation_enabled = False
                mutations.append("cancel-rotation")
                result = {"ARN": secret_arn, "Name":"rds!cluster-MODEL"}
            elif target == "DescribeSecret":
                result = {"ARN":secret_arn,"Name":"rds!cluster-MODEL", "RotationEnabled":rotation_enabled}
            else:
                unexpected.append(target)
                self.send_error(400)
                return
            output = json.dumps(result).encode()
            content_type = "application/x-amz-json-1.1"
        else:
            data = {key: value[0] for key, value in urllib.parse.parse_qs(body.decode()).items()}
            action = data["Action"]
            if action in ("CreateDBCluster", "ModifyDBCluster"):
                if refuse_database:
                    denied_operations.append(action)
                    self.send_response(403)
                    self.end_headers()
                    self.wfile.write(b'<ErrorResponse><Error><Code>AccessDenied</Code><Message>fixture denied</Message></Error></ErrorResponse>')
                    return
                assert "MasterUserPassword" not in data, "Terraform sent a password"
                if action == "CreateDBCluster":
                    assert data.get("ManageMasterUserPassword") == "true"
                    assert cluster is None, "duplicate cluster create"
                    rotate_managed_secret()
                if data.get("ManageMasterUserPassword") == "true" and not managed:
                    managed = True
                    rotate_managed_secret()
                backup_days = data.get("BackupRetentionPeriod", backup_days)
                cluster = True
                mutations.append(action)
                result = cluster_xml()
            elif action == "CreateDBInstance":
                assert cluster and not instance
                instance = True
                instance_minor_upgrade = data["AutoMinorVersionUpgrade"]
                rotation_enabled = True
                mutations.append(action)
                result = instance_xml()
            elif action == "DescribeDBInstances":
                result = "<DBInstances>" + (instance_xml() if instance else "") + "</DBInstances>"
            elif action == "DescribeDBClusters":
                result = "<DBClusters>" + (cluster_xml() if cluster else "") + "</DBClusters>"
            elif action == "DescribeGlobalClusters":
                result = "<GlobalClusters/>"
            elif action == "ListTagsForResource":
                result = "<TagList>" + "".join(f"<Tag><Key>{k}</Key><Value>{v}</Value></Tag>" for k, v in [
                    ("managed-by", "setup"), ("deployment", "credential-model"), ("resource", "db"), ("resource-type", "postgres")]) + "</TagList>"
            else:
                unexpected.append(action)
                self.send_error(400)
                return
            output = f'<{action}Response xmlns="http://rds.amazonaws.com/doc/2014-10-31/"><{action}Result>{result}</{action}Result><ResponseMetadata><RequestId>fixture</RequestId></ResponseMetadata></{action}Response>'.encode()
            content_type = "text/xml"
        self.send_response(200)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(output)))
        self.end_headers()
        self.wfile.write(output)


def rotate_managed_secret():
    # Model the documented RDS ownership boundary, not independent secret writes.
    global current, db_password
    current = str(len(versions) + 1)
    db_password = secrets.token_hex(24)
    versions[current] = db_password


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
url = f"http://127.0.0.1:{server.server_port}"
(root / "provider.tf").write_text('''terraform {
  required_version = ">= 1.11.0"
  required_providers {
    aws = { source = "registry.terraform.io/hashicorp/aws", version = "= 6.70.0" }
    random = { source = "registry.terraform.io/hashicorp/random", version = "= 3.7.2" }
  }
}
locals { resource_prefix = "credential-model" }
provider "aws" {
  region = "us-east-1"
  access_key = "fixture"
  secret_key = "fixture"
  skip_credentials_validation = true
  skip_metadata_api_check = true
  skip_requesting_account_id = true
  skip_region_validation = true
  max_retries = 0
  endpoints { secretsmanager = "URL" rds = "URL" }
}
'''.replace("URL", url).replace('endpoints { secretsmanager =', 'endpoints {\n secretsmanager =').replace(' rds =', '\n rds =').replace('" }\n}', '"\n }\n}'))


def execute(name, command, fails=False):
    result = subprocess.run(command, cwd=root, env=env, capture_output=True, timeout=180)
    # Failure diagnostics may contain provider request values. Never print them.
    if (result.returncode != 0) != fails:
        (root / "failure.log").write_bytes(result.stdout + result.stderr)
        raise AssertionError(f"{name}: unexpected exit {result.returncode}; protected fixture failure.log")
    for path in root.glob("*.tfstate*"):
        payload = path.read_bytes()
        assert all(value.encode() not in payload for value in versions.values()), f"secret persisted in {path.name}"
    print(name, "passed", flush=True)


def run(name, *args, fails=False):
    execute(name, [cli, *args, "-no-color"], fails)



try:
    run("init", "init", "-backend=false", "-input=false")
    if legacy_migration:
        cluster = True
        instance = True
        instance_minor_upgrade = "true"
        db_password = secrets.token_hex(24)
        versions["legacy"] = db_password
        current = "legacy"
        # Model the original-graph metadata upgrade before changing credentials.
        generated = (root / "main.tf").read_text()
        original = (root / "previous-main.txt").read_text()
        assert original != generated
        (root / "main.tf").write_text(original)
        # Native imports establish exact provider IDs without fabricating state.
        run("import existing cluster", "import", "aws_rds_cluster.db", "credential-model-db")
        run("import existing writer", "import", "aws_rds_cluster_instance.db", "credential-model-db-1")
        run("install original-graph inventory", "apply", "-auto-approve", "-target=terraform_data.retention_inventory")
        receipt = root / "legacy-recovery"
        saved = json.loads(subprocess.check_output([cli, "state", "pull"], cwd=root, env=env))
        # Preserve the three addresses emitted by the previous password generator.
        # The fixture contains synthetic credentials only. Native AWS/random schemas
        # validate the installed state and plan the transition without cloud reads.
        schemas = json.loads(subprocess.check_output([cli, "providers", "schema", "-json"], cwd=root, env=env))["provider_schemas"]
        legacy_secret = "arn:aws:secretsmanager:us-east-1:123456789012:secret:credential-model-db-credentials-abcdef"
        legacy_resources = [
            ("random", "random_password", {"id":"none", "length":60, "special":False, "result":"synthetic-installed-password"}),
            ("aws", "aws_secretsmanager_secret", {"id":legacy_secret, "arn":legacy_secret, "name":"credential-model-db-credentials", "recovery_window_in_days":0}),
            ("aws", "aws_secretsmanager_secret_version", {"id":legacy_secret + "|synthetic-version", "secret_id":legacy_secret, "version_id":"synthetic-version", "secret_string":"synthetic-installed-password"}),
        ]
        for provider, kind, attrs in legacy_resources:
            provider_id = f"registry.terraform.io/hashicorp/{provider}"
            saved["resources"].append({"mode":"managed", "type":kind, "name":"db",
                "provider":f'provider["{provider_id}"]', "instances":[{
                    "schema_version":schemas[provider_id]["resource_schemas"][kind]["version"],
                    "attributes":attrs, "sensitive_attributes":[]}]})
        (root / "installed.tfstate").write_text(json.dumps(saved))
        run("install previous credential addresses", "state", "push", "-force", "installed.tfstate")
        unsafe = re.sub(r"removed \{.*?\n\}", "", generated, flags=re.S)
        (root / "main.tf").write_text(unsafe)
        run("before upgrade plan", "plan", "-refresh=false", "-out=before.tfplan")
        before = json.loads(subprocess.check_output([cli,"show","-json","before.tfplan"], cwd=root, env=env))
        before_actions = {r["address"]:r["change"]["actions"] for r in before["resource_changes"]}
        assert all(before_actions[f"{kind}.db"] == ["delete"] for _, kind, _ in legacy_resources)
        (root / "main.tf").write_text(generated)
        run("safe upgrade plan", "plan", "-refresh=false", "-out=safe.tfplan")
        safe = json.loads(subprocess.check_output([cli,"show","-json","safe.tfplan"], cwd=root, env=env))
        actions = {r["address"]:r["change"]["actions"] for r in safe["resource_changes"]}
        assert all(actions[f"{kind}.db"] == ["forget"] for _, kind, _ in legacy_resources), actions
        assert actions["aws_rds_cluster.db"] == ["update"], actions
        assert not any("delete" in a for a in actions.values()), actions
        excerpt = subprocess.check_output([cli,"show","-no-color","safe.tfplan"], cwd=root, env=env).decode()
        # Save only a credential-free excerpt for the upgrade evidence.
        excerpt = "\n".join(line for line in excerpt.splitlines() if
            "will no longer be managed" in line or "will be updated in-place" in line or
            "manage_master_user_password" in line or line.startswith("Plan:"))
        evidence = os.environ.get("UPGRADE_PLAN_EVIDENCE")
        if evidence:
            pathlib.Path(evidence).write_text(excerpt + "\n")
        # Forgetting is local state work; preserve the separate denied ModifyDBCluster
        # recovery journey below without sending legacy-secret reads to its model.
        for _, kind, _ in legacy_resources:
            run("detach legacy credential address", "state", "rm", f"{kind}.db")
        run("managed migration plan", "plan", "-out=rotation.tfplan", "-var=backup_days=8")
        refuse_database = True
        run("denied managed migration", "apply", "rotation.tfplan", fails=True)
        assert denied_operations == ["ModifyDBCluster"]
        assert not managed and db_password == versions["legacy"]
        refuse_database = False
        run("legacy retry plan", "plan", "-out=legacy-retry.tfplan", "-var=backup_days=8")
        saved_cluster = next(r for r in saved["resources"] if r.get("mode") == "managed" and r["type"] == "aws_rds_cluster")
        identity = saved_cluster["instances"][0]["attributes"]["id"]
        assert identity == "credential-model-db"
        run("remove only cluster state for exact-ID reimport", "state", "rm", "-lock-timeout=60s", "aws_rds_cluster.db")
        run("restore exact installed cluster", "import", "aws_rds_cluster.db", identity)
        run("reimported upgrade plan", "plan", "-out=reimported.tfplan", "-var=backup_days=8")
        run("recover managed migration after exact-ID import", "apply", "reimported.tfplan")
        assert managed and current != "legacy" and db_password == versions[current]
        assert mutations.count("CreateDBCluster") == 0 and mutations.count("CreateDBInstance") == 0
        assert not unexpected and not reads
        print("legacy managed migration recovered without replacement", flush=True)
        passed = True
        sys.exit(0)
    refuse_database = True
    run("denied create", "apply", "-auto-approve", fails=True)
    assert cluster is None and not versions
    refuse_database = False
    run("recover database create", "apply", "-auto-approve")
    first = current
    assert db_password == versions[first] and mutations.count("CreateDBCluster") == 1
    assert instance and not rotation_enabled
    before = list(mutations)
    run("unchanged apply", "apply", "-auto-approve")
    assert mutations == before
    receipt = root / "recovery"
    run("database update plan", "plan", "-out=rotation.tfplan", "-var=backup_days=8")
    refuse_database = True
    run("denied database update", "apply", "rotation.tfplan", fails=True)
    assert current == first and db_password == versions[first] and backup_days == "7"
    refuse_database = False
    run("retry plan after denied update", "plan", "-out=retry.tfplan", "-var=backup_days=8")
    run("recover database update", "apply", "retry.tfplan")
    assert backup_days == "8" and current == first and db_password == versions[first]
    rotate_managed_secret()
    assert current != first and db_password != versions[first]
    before = list(mutations)
    run("refresh", "apply", "-refresh-only", "-auto-approve", "-var=backup_days=8")
    run("saved plan", "plan", "-out=proof.tfplan", "-var=backup_days=8")
    plan = subprocess.run([cli, "show", "-json", "proof.tfplan"], cwd=root, env=env, capture_output=True, check=True).stdout
    assert all(value.encode() not in plan for value in versions.values()), "secret in saved plan"
    for path in root.rglob("*.tfplan"):
        with zipfile.ZipFile(path) as archive:
            for member in archive.namelist():
                payload = archive.read(member)
                assert all(value.encode() not in payload for value in versions.values()), "secret in decompressed saved plan"
    assert not rotation_enabled
    assert mutations == before and not reads, "Terraform touched managed credentials"
    assert not unexpected, unexpected
    assert mutations.count("CreateDBCluster") == 1 and mutations.count("ModifyDBCluster") == 1
    passed = True
finally:
    if passed:
        (root / "receipt.json").write_text(json.dumps({
            "cli": cli, "provider": "aws6.70.0", "legacy_migration": legacy_migration,
            "mutations": mutations, "reads": reads, "denied_operations": denied_operations,
            "scope": "Native generated managed credential graph; not cloud or SQL authentication proof",
        }, indent=2) + "\n")
        if export := os.environ.get("ALIEN_TEST_TERRAFORM_ARTIFACT_DIR"):
            name = "aws-managed-legacy" if legacy_migration else "aws-managed-fresh"
            shutil.copytree(root, pathlib.Path(export) / name, ignore=shutil.ignore_patterns(".terraform"))
    server.shutdown()
    server.server_close()
