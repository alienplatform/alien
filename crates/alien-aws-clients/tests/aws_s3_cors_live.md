# S3 browser CORS qualification

This opt-in test creates one private, uniquely named test bucket, runs the Rust
client to set, replace and clear its CORS configuration, and independently checks
AWS metadata and browser GET/HEAD/range responses. Each probe uses a new object
path to avoid cached responses. It checks two allowed origins, origin removal,
a refused origin, and the exposed ETag, Content-Range and Accept-Ranges headers.
Each phase allows at most 90 seconds of read-only configuration/CORS observations
for [S3 bucket configuration propagation](https://docs.aws.amazon.com/AmazonS3/latest/userguide/Welcome.html#ConsistencyModel).
Every observation is recorded. Mutations are sent once, and authentication or
unexpected response errors fail immediately.

Use credentials for a dedicated test account with permission to create, tag,
configure and delete this bucket and write/read/delete its test objects. This
qualifies the client with the supplied credentials; it does not qualify generated
runtime grants, the controller, or the Terraform installer.

From the repository root, with Python `boto3` and `requests` installed:

```bash
# Select your normal test credentials, for example through AWS_PROFILE.
export AWS_PROFILE=my-test-profile
umask 077
cargo test -p alien-aws-clients --test aws_s3_cors_http_tests \
  --no-run --message-format=json > /tmp/s3-cors-build.jsonl
python3 crates/alien-aws-clients/tests/aws_s3_cors_live.py \
  --build-log /tmp/s3-cors-build.jsonl \
  --evidence-dir /tmp/s3-cors-evidence \
  --expected-account YOUR_TEST_ACCOUNT_ID \
  --region us-east-2
```

The evidence directory must not exist. The script verifies the caller's account
before creating anything. It writes temporary credentials with mode 0600 only
inside that private directory, removes them after each client call and on exit,
and records no signed object URLs. For an environment with a trusted TLS proxy,
pass `--ca-bundle /path/to/system-ca-bundle.pem`; certificate-chain and hostname
validation remain enabled.

A `finally` block checks the ownership tag, removes only the recorded test objects,
deletes the bucket and independently requires HeadBucket 404. Check `receipt.json`
for both `result: PASS` and completed cleanup. A process kill cannot run `finally`:
use the receipt's exact bucket, verify its `cors-test-bucket` tag equals its name,
and remove its recorded test objects and bucket with the same account. Never use
a wildcard account-wide cleanup. Keep failed receipts and logs; use a new evidence
directory for a new attempt.
