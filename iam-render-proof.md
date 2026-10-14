# IAM inline policy render proof

Actual `alien render` comparison: original parent `aa42e2f38` versus feature `f60521756`. Fixtures contain only synthetic S3 object ARNs. No cloud credentials, cloud API calls, or apply were used.

| Scenario | Parent CFN / TF | Feature CFN / TF |
|---|---|---|
| 400 explicit object grants | exit 0, artifacts emitted | exit 1, no artifact files |
| 10 explicit object grants | exit 0, artifacts emitted | exit 0, artifacts emitted |

Feature errors report at least 18,900 inline characters for `ExecutionSaRole` (CFN) and 21,317 for `execution_sa` (TF), against the fixed aggregate limit 10,240. Terraform 1.9.8 console evaluated the parent generator's exact literal `jsonencode` expression: 21,317 oversized / 647 small. CFN retains a conservative lower bound when intrinsics are unresolved.

This proves CLI rendering refusal before artifacts are written, including without `--check-quotas`. It does not prove native cloud application, direct deployment-wide aggregation, update overlap with old inline policies, or an account's applied managed attachment quota. Dynamic documents and unresolved attachments remain limitations of the conservative guards.
