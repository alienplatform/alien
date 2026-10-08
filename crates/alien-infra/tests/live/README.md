# Live AWS vault permission update

Run from the repository root with `AWS_TARGET_ACCOUNT_ID`, `AWS_TARGET_REGION`,
`AWS_TARGET_ACCESS_KEY_ID`, and `AWS_TARGET_SECRET_ACCESS_KEY` set for an authorized
test account. Install boto3, make Cargo available, then run:

```sh
python3 crates/alien-infra/tests/live/aws_vault_permission_update.py
```

The runner creates two uniquely named IAM roles and two synthetic SecureString
parameters, proves the consumer is initially denied, runs the real vault update
through StackExecutor, verifies a decrypted read succeeds, and verifies cross-role
and cross-namespace reads stay denied. It deletes its fixtures even when a check
fails and verifies their absence. The role fixtures stand in for already-installed
service accounts; this is controller integration coverage, not worker/container
startup or a complete deployment lifecycle. A sanitized receipt is saved under
`/tmp/vault-update-live-receipt.json`.
