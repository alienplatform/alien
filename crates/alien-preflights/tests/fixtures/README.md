# Legacy management policy baselines

Both fixtures were compiled independently on pre-boundary main commit
`010e369fc5df2d5cbc1bc3a48541003e4310c305`, with fake account IDs and no boundary
context. Do not regenerate them from the current branch to accept a test failure.

To reproduce `legacy_automatic_iam_policy.json`, use the stack, configuration and
context from `../aws_automatic_permissions_boundary.rs`. Run
`ManagementPermissionProfileMutation` for `automatic_stack()`, resolve
`management_identity_global_refs`, and compile with `AwsRuntimePermissionsGenerator`
at `BindingTarget::Stack`. Retain only `iam:` actions and drop empty statements.

For `legacy_manual_management_policies.json`, copy `../support/manual_management.rs`
to that commit. Run `compile_manual_management(false)` and
`compile_manual_management(true)` from a Tokio test; serialize under `extend` and
`override`. In global scope, retain actions whose service pattern matches `iam`
(including `*`) and drop empty statements. Keep all resource-scoped statements.

Tests compare grants and denies in both directions; manual profiles additionally
exercise scoped data access. Global non-IAM grants are excluded because tagging
restrictions intentionally changed. These fixtures establish compilation
compatibility, not controller attachment or live upgrade behavior.
