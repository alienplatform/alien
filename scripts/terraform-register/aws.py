#!/usr/bin/env python3
"""Register a protected Terraform module on real AWS through its outputs.

A protected module carries no registration provider (that code would run with the customer's
credentials during `terraform apply`); after apply, `alien-deploy register --import terraform`
reads `terraform output -json` and sends the import request. This renders `app/` (one storage
bucket) as the protected `terraform-aws` module with the `render_setup` example of
alien-cli-common, pinned to this account as the machine-image publisher and to
`<prefix>-verifier@<digest>` as the verifier image (a small public image copied into ECR, as in
scripts/aws-setup-locks), applies it, runs the register command with `--dry-run` for both
destinations (a standalone manager and the hosted platform), and compares the request field by
field with what the module itself computed for registration: the `terraform_data`
`deployment_registration` input in the state, which holds the same locals the registration
resource of a self-registering module sent. Then destroys everything and lists what still
carries the run tag or the prefix; the run fails if anything does.

  cargo build -p alien-cli-common --example render_setup
  cargo build -p alien-deploy-cli --bin alien-deploy
  python3 scripts/terraform-register/aws.py \\
      --renderer target/debug/examples/render_setup --deploy-cli target/debug/alien-deploy \\
      --prefix tfr-check --tag purpose=tfr-check --evidence /tmp/tfr-evidence

Requires boto3, terraform, and administrator credentials for a disposable test account in the
environment.
"""

import argparse
import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOCKS = HERE.parent / "aws-setup-locks"
FINGERPRINT = "SHA256:" + "9f2c3b1a77d0e5c2" * 4


def load_locks():
    spec = importlib.util.spec_from_file_location("aws_setup_locks", LOCKS / "check.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(command, **kwargs):
    print("+", " ".join(str(part) for part in command), flush=True)
    return subprocess.run([str(part) for part in command], check=True, **kwargs)


def compare(request, outputs, state_input, region):
    """Each import request field against what the module computed for registration."""
    decoded = lambda name: json.loads(outputs[name]["value"])
    rows = [
        ("deploymentName", request["deploymentName"], state_input["name"]),
        ("resourcePrefix", request["resourcePrefix"], state_input["resource_prefix"]),
        ("platform", request["platform"], state_input["platform"]),
        ("region", request["region"], region),
        ("setupTarget", request["setupTarget"], state_input["setup_target"]),
        ("setupImportFormatVersion", request["setupImportFormatVersion"], state_input["setup_import_format_version"]),
        ("setupFingerprint", request["setupFingerprint"], state_input["setup_fingerprint"]),
        ("setupFingerprintVersion", request["setupFingerprintVersion"], state_input["setup_fingerprint_version"]),
        ("managementConfig", request["managementConfig"], state_input["management_config"]),
        ("stackSettings", request["stackSettings"], state_input["stack_settings"]),
        ("resources", request["resources"], state_input["resources"]),
        ("inputValues", request.get("inputValues", {}), state_input["inputValues"]),
        ("sourceKind", request.get("sourceKind"), "terraform"),
        ("releaseId", request.get("releaseId"), None),
        ("managementConfig (output)", request["managementConfig"], decoded("deployment_management_config")),
        ("stackSettings (output)", request["stackSettings"], decoded("deployment_stack_settings")),
    ]
    return [{"field": field, "request": got, "module": want, "ok": got == want} for field, got, want in rows]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--renderer", required=True)
    parser.add_argument("--deploy-cli", required=True)
    parser.add_argument("--prefix", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--region", default="us-east-1")
    parser.add_argument("--evidence", required=True)
    args = parser.parse_args()

    locks = load_locks()
    check = locks.Run(args)
    evidence = Path(args.evidence)
    work = evidence / "module"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    env = dict(os.environ, AWS_REGION=args.region, AWS_DEFAULT_REGION=args.region)
    applied = False
    failures = []
    try:
        digest = check.copy_verifier_image()
        # The module adds the account's registry host to the pinned repository.
        image = f"{args.prefix}-verifier@{digest}"
        caller = check.caller_role()

        settings = {"setupManagement": "manual", "signing": {"fingerprint": FINGERPRINT, "channel": "production"}}
        (evidence / "settings.json").write_text(json.dumps(settings))
        rendered = evidence / "module.json"
        run([args.renderer, "terraform-aws", LOCKS / "app" / "alien.json", evidence / "settings.json",
             rendered, check.account, image])
        for path, contents in json.loads(rendered.read_text()).items():
            target = work / path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(contents)

        env.update({
            "TF_VAR_name": args.prefix,
            "TF_VAR_resource_prefix": args.prefix,
            "TF_VAR_aws_region": args.region,
            "TF_VAR_managing_role_arn": caller,
            "TF_IN_AUTOMATION": "1",
        })
        run(["terraform", f"-chdir={work}", "init", "-input=false", "-no-color"], env=env)
        applied = True
        run(["terraform", f"-chdir={work}", "apply", "-auto-approve", "-input=false", "-no-color"], env=env)

        outputs = json.loads(run(["terraform", f"-chdir={work}", "output", "-json"], env=env,
                                 capture_output=True, text=True).stdout)
        state = json.loads(run(["terraform", f"-chdir={work}", "show", "-json"], env=env,
                               capture_output=True, text=True).stdout)
        registration = next(
            resource for resource in state["values"]["root_module"]["resources"]
            if resource["address"] == "terraform_data.deployment_registration"
        )
        state_input = registration["values"]["input"]
        (evidence / "outputs.json").write_text(json.dumps(outputs, indent=2) + "\n")
        (evidence / "registration-input.json").write_text(json.dumps(state_input, indent=2) + "\n")

        requests = {}
        for destination, flags in {
            "standalone": ["--manager-url", "http://127.0.0.1:9"],
            "platform": ["--base-url", "https://api.example.invalid"],
        }.items():
            printed = run([args.deploy_cli, "register", "--import", "terraform", "--dir", work,
                           "--token", "dg_dry_run", *flags, "--dry-run"], env=env,
                          capture_output=True, text=True).stdout
            requests[destination] = json.loads(printed)
            (evidence / f"request-{destination}.json").write_text(json.dumps(requests[destination], indent=2) + "\n")

        rows = compare(requests["standalone"], outputs, state_input, args.region)
        forward = requests["platform"]["source"]
        for field in ["deploymentName", "resourcePrefix", "platform", "region", "setupTarget",
                      "setupImportFormatVersion", "setupFingerprint", "setupFingerprintVersion",
                      "managementConfig", "stackSettings", "resources", "sourceKind"]:
            rows.append({"field": f"forward.{field}", "request": forward.get(field),
                         "module": requests["standalone"].get(field),
                         "ok": forward.get(field) == requests["standalone"].get(field)})
        rows.append({"field": "forward.releaseId", "request": forward.get("releaseId"), "module": None,
                     "ok": "releaseId" not in forward})
        rows.append({"field": "forward.mode", "request": requests["platform"].get("mode"), "module": "forward",
                     "ok": requests["platform"].get("mode") == "forward"})
        parameter = json.loads(check.ssm.get_parameter(Name=f"/signed/{args.prefix}/fingerprint")["Parameter"]["Value"])
        rows.append({"field": "settings parameter", "request": parameter,
                     "module": settings["signing"],
                     "ok": (parameter["fingerprint"], parameter["channel"]) == (FINGERPRINT, "production")})
        signing = requests["standalone"]["stackSettings"].get("signing")
        rows.append({"field": "stackSettings.signing", "request": signing, "module": settings["signing"],
                     "ok": signing == settings["signing"]})
        (evidence / "comparison.json").write_text(json.dumps(rows, indent=2, default=str) + "\n")
        for row in rows:
            print(f"{'ok  ' if row['ok'] else 'FAIL'} {row['field']}", flush=True)
        failures = [row["field"] for row in rows if not row["ok"]]
    except Exception as error:  # noqa: BLE001 - cleaned up and audited below, then failed
        print(f"FAIL run: {error!r}", flush=True)
        failures = ["run"]
    finally:
        if applied:
            # Setup-owned buckets refuse destroy; detach the bucket, destroy the rest, then delete it.
            subprocess.run(["terraform", f"-chdir={work}", "state", "rm", "aws_s3_bucket.assets"], env=env)
            subprocess.run(["terraform", f"-chdir={work}", "destroy", "-auto-approve", "-input=false",
                            "-no-color"], env=env, check=False)
            s3 = check.session.client("s3")
            try:
                s3.delete_bucket(Bucket=f"{args.prefix}-assets")
            except Exception as error:  # noqa: BLE001 - reported by the audit below
                print(f"bucket delete: {error}", flush=True)
        for cleanup in reversed(check.cleanups):
            try:
                cleanup()
            except Exception as error:  # noqa: BLE001 - reported by the audit below
                print(f"cleanup: {error}", flush=True)
    leftovers = audit(check, args.prefix)
    (evidence / "cleanup-audit.json").write_text(json.dumps(leftovers, indent=2, default=str) + "\n")
    if leftovers:
        print(f"LEFT BEHIND: {leftovers}", flush=True)
    if failures or leftovers:
        sys.exit(1)
    print("PASS", flush=True)


def audit(check, prefix):
    """Everything named with the prefix that is still there."""
    left = []
    session = check.session
    s3 = session.client("s3")
    left += [b["Name"] for b in s3.list_buckets()["Buckets"] if b["Name"].startswith(prefix)]
    left += [f["FunctionName"] for f in check.lam.list_functions()["Functions"] if f["FunctionName"].startswith(prefix)]
    for page in check.iam.get_paginator("list_roles").paginate():
        left += [r["RoleName"] for r in page["Roles"] if r["RoleName"].startswith(prefix)]
    for page in check.iam.get_paginator("list_policies").paginate(Scope="Local"):
        left += [p["PolicyName"] for p in page["Policies"] if p["PolicyName"].startswith(prefix)]
    left += [p["Name"] for p in check.ssm.describe_parameters(
        ParameterFilters=[{"Key": "Name", "Option": "BeginsWith", "Values": [f"/signed/{prefix}"]}])["Parameters"]]
    left += [r["repositoryName"] for r in check.ecr.describe_repositories()["repositories"]
             if r["repositoryName"].startswith(prefix)]
    return left


if __name__ == "__main__":
    main()
