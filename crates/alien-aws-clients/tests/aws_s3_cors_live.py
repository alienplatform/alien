#!/usr/bin/env python3
"""Run the opt-in Rust S3 CORS client against one owned bucket, then erase all fixtures."""

import argparse
import hashlib
import json
import os
import pathlib
import secrets
import subprocess
import time
import boto3, requests
from botocore.config import Config
from botocore.exceptions import ClientError

parser = argparse.ArgumentParser(
    description="Qualify S3 browser CORS on one disposable bucket and erase it afterward"
)
parser.add_argument("--build-log", type=pathlib.Path, required=True)
parser.add_argument("--evidence-dir", type=pathlib.Path, required=True)
parser.add_argument("--expected-account", required=True)
parser.add_argument("--region", required=True)
parser.add_argument("--ca-bundle", type=pathlib.Path)
args = parser.parse_args()
d = args.evidence_dir
d.mkdir(mode=0o700, exist_ok=False)
records = [
    json.loads(line)
    for line in args.build_log.read_text().splitlines()
    if line.startswith("{")
]
assert records[-1].get("success") is True, "build must finish first"
binary = next(
    (
        x["executable"]
        for x in records
        if x.get("executable")
        and x.get("target", {}).get("name") == "aws_s3_cors_http_tests"
    )
)
region = args.region
bucket = "test-cors-" + secrets.token_hex(8)
session = boto3.Session(region_name=region)
assert session.client("sts").get_caller_identity()["Account"] == args.expected_account
credentials = session.get_credentials().get_frozen_credentials()
s3 = session.client(
    "s3",
    endpoint_url=f"https://s3.{region}.amazonaws.com",
    config=Config(
        signature_version="s3v4",
        connect_timeout=10,
        read_timeout=20,
        retries={"max_attempts": 0},
    ),
)
receipt = {
    "bucket": bucket,
    "region": region,
    "identityVerified": True,
    "boundary": "caller-supplied test credential; generated runtime grants not qualified",
    "binarySha256": hashlib.sha256(pathlib.Path(binary).read_bytes()).hexdigest(),
    "caBundleSha256": (
        hashlib.sha256(args.ca_bundle.read_bytes()).hexdigest()
        if args.ca_bundle
        else None
    ),
    "checks": [],
    "cleanup": "pending",
}


def save():
    (d / "receipt.json").write_text(json.dumps(receipt, indent=2))


fixture = d / "credentials-input.json"
keys = []
receipt["objectKeys"] = keys
created = False
passed = False
save()
try:
    try:
        s3.head_bucket(Bucket=bucket)
        raise AssertionError("bucket name already exists")
    except ClientError as e:
        assert e.response["ResponseMetadata"]["HTTPStatusCode"] == 404
    s3.create_bucket(
        Bucket=bucket, CreateBucketConfiguration={"LocationConstraint": region}
    )
    created = True
    s3.put_bucket_tagging(
        Bucket=bucket,
        Tagging={"TagSet": [{"Key": "cors-test-bucket", "Value": bucket}]},
    )
    for phase, origins in [
        ("set", ["https://app.example.com", "https://preview.example.com"]),
        ("replace", ["https://replacement.example.com"]),
        ("clear", []),
    ]:
        payload = {
            "bucket": bucket,
            "origins": origins,
            "config": {
                "accountId": args.expected_account,
                "region": region,
                "credentials": {
                    "type": "accessKeys",
                    "access_key_id": credentials.access_key,
                    "secret_access_key": credentials.secret_key,
                    "session_token": credentials.token,
                },
            },
        }
        if args.ca_bundle:
            payload["caBundleFile"] = str(args.ca_bundle.resolve())
        with os.fdopen(
            os.open(fixture, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600), "w"
        ) as f:
            json.dump(payload, f)
        with (d / (phase + "-client.log")).open("w") as log:
            rc = subprocess.run(
                [
                    binary,
                    "--ignored",
                    "--exact",
                    "apply_read_cors_to_owned_test_bucket",
                    "--nocapture",
                ],
                env=dict(os.environ, ALIEN_S3_CORS_LIVE_FIXTURE=str(fixture)),
                stdout=log,
                stderr=subprocess.STDOUT,
                timeout=90,
            ).returncode
        fixture.unlink()
        assert rc == 0, (phase, "client exit", rc)
        # Bucket configuration is eventually consistent. Observe only, with a fixed
        # per-phase deadline; never repeat the mutation or mask authentication errors.
        # https://docs.aws.amazon.com/AmazonS3/latest/userguide/Welcome.html#ConsistencyModel
        deadline = time.monotonic() + 90
        while True:
            try:
                rules = s3.get_bucket_cors(Bucket=bucket)["CORSRules"]
            except ClientError as error:
                if error.response["Error"]["Code"] != "NoSuchCORSConfiguration":
                    raise
                rules = []
            receipt["checks"].append(
                {"phase": phase, "kind": "configuration", "rules": rules}
            )
            save()
            matches = (
                (not rules)
                if not origins
                else (
                    len(rules) == 1
                    and rules[0]["AllowedOrigins"] == origins
                    and set(rules[0]["AllowedMethods"]) == {"GET", "HEAD"}
                )
            )
            if matches:
                break
            assert time.monotonic() < deadline, (
                phase,
                "CORS readback did not converge",
            )
            time.sleep(2)
        for origin in [
            "https://app.example.com",
            "https://preview.example.com",
            "https://replacement.example.com",
            "https://refused.example.com",
        ]:
            for method in ["GET", "HEAD"]:
                key = phase + "-" + secrets.token_hex(8)
                keys.append(key)
                s3.put_object(
                    Bucket=bucket,
                    Key=key,
                    Body=b"storage-live-proof",
                    CacheControl="no-store",
                )
                save()
                attempt = 0
                while True:
                    response = requests.options(
                        f"https://{bucket}.s3.{region}.amazonaws.com/{key}-preflight-{attempt}",
                        headers={
                            "Origin": origin,
                            "Access-Control-Request-Method": method,
                            "Access-Control-Request-Headers": "range",
                        },
                        timeout=30,
                    )
                    allowed = origin in origins
                    receipt["checks"].append(
                        {
                            "phase": phase,
                            "kind": "OPTIONS",
                            "method": method,
                            "origin": origin,
                            "status": response.status_code,
                            "headers": dict(response.headers),
                            "body": response.text[:1200],
                        }
                    )
                    save()
                    assert response.status_code in (200, 403), (
                        "unexpected preflight status",
                        response.status_code,
                    )
                    preflight_matches = response.status_code == (
                        200 if allowed else 403
                    ) and (
                        (response.headers.get("Access-Control-Allow-Origin") == origin)
                        == allowed
                    )
                    url = s3.generate_presigned_url(
                        "get_object" if method == "GET" else "head_object",
                        Params={"Bucket": bucket, "Key": key},
                        ExpiresIn=180 + attempt,
                    )
                    response = requests.request(
                        method,
                        url,
                        headers={
                            "Origin": origin,
                            **({"Range": "bytes=0-5"} if method == "GET" else {}),
                        },
                        timeout=30,
                    )
                    receipt["checks"].append(
                        {
                            "phase": phase,
                            "kind": method,
                            "origin": origin,
                            "status": response.status_code,
                            "headers": dict(response.headers),
                        }
                    )
                    save()
                    assert response.status_code == (206 if method == "GET" else 200), (
                        phase,
                        origin,
                        method,
                        response.status_code,
                    )
                    cors_matches = (
                        response.headers.get("Access-Control-Allow-Origin") == origin
                    ) == allowed
                    if not (preflight_matches and cors_matches):
                        assert time.monotonic() < deadline, (
                            phase,
                            origin,
                            method,
                            "browser CORS did not converge",
                        )
                        time.sleep(2)
                        attempt += 1
                        continue
                    if allowed:
                        exposed = {
                            x.strip().lower()
                            for x in response.headers.get(
                                "Access-Control-Expose-Headers", ""
                            ).split(",")
                        }
                        assert {"etag", "content-range", "accept-ranges"} <= exposed
                        assert (
                            response.headers["ETag"]
                            and response.headers["Accept-Ranges"] == "bytes"
                        )
                        if method == "GET":
                            assert (
                                response.content == b"storag"
                                and response.headers["Content-Range"] == "bytes 0-5/18"
                            )
                    break
        print("PASS", phase, "readback and browser matrix", flush=True)
    passed = True
    receipt["result"] = "PASS"
    save()
except Exception as e:
    receipt["result"] = "FAIL"
    receipt["errorType"] = type(e).__name__
    receipt["assertion"] = str(e) if isinstance(e, AssertionError) else None
    save()
    print("FAIL", type(e).__name__, flush=True)
finally:
    fixture.unlink(missing_ok=True)
    if created:
        tags = s3.get_bucket_tagging(Bucket=bucket)["TagSet"]
        assert {"Key": "cors-test-bucket", "Value": bucket} in tags
        objects = s3.list_objects_v2(Bucket=bucket).get("Contents", [])
        assert {x["Key"] for x in objects} <= set(keys)
        if objects:
            s3.delete_objects(
                Bucket=bucket, Delete={"Objects": [{"Key": x["Key"]} for x in objects]}
            )
        s3.delete_bucket(Bucket=bucket)
    try:
        s3.head_bucket(Bucket=bucket)
        raise AssertionError("bucket remains")
    except ClientError as e:
        assert e.response["ResponseMetadata"]["HTTPStatusCode"] == 404
    receipt["cleanup"] = "independent HeadBucket404; credentials-input removed"
    save()
    print("PASS cleanup404", flush=True)
raise SystemExit(0 if passed else 1)
