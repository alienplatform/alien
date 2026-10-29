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

# Legacy manual management policies

`legacy_manual_management_policies.json` was compiled independently on the same
main commit, `010e369fc5df2d5cbc1bc3a48541003e4310c305`. Its input and compiler are
in `../support/manual_management.rs`: manual setup, Frozen storage and vault,
a Live worker, explicit resource-scoped data grants, and global worker/container
provision grants. Both Extend and Override profiles run through the real
management-profile mutation. The container grant exercises the backup-role deny
even though the sample has no container. Physical names and account IDs are fake.

To reproduce, copy that support module to the recorded commit and call
`compile_manual_management(false)` and `compile_manual_management(true)` from a
Tokio test. Serialize the results under `extend` and `override`. For the global
`*` scope, retain only actions whose service pattern matches `iam` (including
`*`), then drop empty statements. Retain every statement in resource scopes.
The input deliberately leaves the boundary context unset on both versions.

The regression compares coverage in both directions for each scope. It also
checks actual authorization decisions on the combined policy: existing role
writes work without a boundary; boundary attachment remains unavailable; backup
roles remain protected; the bucket data grant cannot read another bucket.
Global non-IAM grants are excluded because tag restrictions intentionally changed.
This proves compilation compatibility for these manual profiles, not resource
controller attachment, a setup rerun, or a live release upgrade.
