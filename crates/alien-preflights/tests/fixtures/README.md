# Legacy automatic IAM policy

`legacy_automatic_iam_policy.json` was compiled on the pre-boundary main commit
`010e369fc5df2d5cbc1bc3a48541003e4310c305` using the stack, deployment config and
permission context in `aws_automatic_permissions_boundary.rs`.

To reproduce, run `ManagementPermissionProfileMutation` for `automatic_stack()`
on that commit, resolve `management_identity_global_refs`, and compile each set
with `AwsRuntimePermissionsGenerator` at `BindingTarget::Stack`. Serialize the
statements after retaining only `iam:` actions and dropping empty statements.
The baseline generator has no boundary context field. All account IDs are fake.

The test compares coverage in both directions, including explicit denies,
resources and conditions. Statement IDs, ordering and regrouping do not determine
compatibility. This checks the complete combined automatic management policy's
IAM access; automatic mode includes provision sets even for absent resources.
It does not prove manual-profile equivalence or a live release upgrade.

Do not regenerate this fixture from the current branch to accept a test failure.
Changes require a deliberate compatibility review against the recorded baseline.
