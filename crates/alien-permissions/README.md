# alien-permissions

Permission system for Alien. Compiles permission sets from JSONC definitions, evaluates policies, and interpolates cloud-specific IAM variables.

Permission sets are defined in `permission-sets/` as JSONC files, specifying the cloud IAM permissions required for each resource type. Split into management (provisioning) and application (runtime) scopes.

For application queues, use `queue/publish` for producers and `queue/data-read`
for consumers. The consumer set includes acknowledgement and visibility
operations as well as receive. `queue/data-write` remains available for
existing profiles that need its combined send-and-acknowledge access.
An unqualified queue link still receives the older combined grants by default;
use an explicit resource profile to keep a publisher or consumer one-way.

## AWS tagging permissions

Tag writes must target resources selected by the stack's ARN bindings, require
existing deployment ownership, or be authorized only during resource creation.
Request tags alone do not establish ownership of an existing resource.

A resource binding's `${resourceName}` is the assigned or imported physical name.
It need not start with `${stackPrefix}`: CloudFormation-generated and shortened
names are valid too. Preserve these bindings instead of rebuilding names from
logical resource IDs. Account-wide patterns such as `*` or `security-group/*`
need an ownership or creation condition.

The catalog audit in `tests/aws_tag_writes.rs` checks tagging grants,
including wildcard actions and CodeBuild's `UpdateProject`, which also
[replaces project tags](https://docs.aws.amazon.com/codebuild/latest/userguide/how-to-tag-project-update.html).
Review creation and update APIs separately when they
accept tags: IAM can check a dependent tagging action, and a service may reject
or ignore tags when updating an existing resource. For example,
[ACM reimports reject tags](https://docs.aws.amazon.com/acm/latest/APIReference/API_ImportCertificate.html),
and [EventBridge rule updates ignore them](https://docs.aws.amazon.com/eventbridge/latest/APIReference/API_PutRule.html).
Verify these service behaviors with the complete generated policy, including
both successful tagged creation and denied foreign-resource tag changes.

## Core Types

- `PermissionContext` — Builder for cloud permission variable context (AWS account/region, GCP project, Azure subscription, etc.)
- `VariableInterpolator` — Cross-cloud permission variable interpolation
- `get_permission_set()` / `list_permission_set_ids()` — Permission set registry operations

## AWS role permissions boundaries

AWS setup creates `${stackPrefix}-deployment-management-boundary` alongside the management
identity's policies. Direct setup, CloudFormation and Terraform use
`aws_stack_permissions_boundary` and record both the installed policy ARN and
its document. Setup owns creation, updates and deletion of this managed policy.
Management's generated grants do not permit editing it or removing a role's
permissions boundary.

The document limits identity-policy permissions for roles management writes:

- Name-based runtime catalog bindings contribute stack-prefixed ARN patterns.
  Resource-specific names widen to the deployment prefix. One `Allow` statement
  permits actions other than `iam:*` on these patterns.
- An existing deployment ownership tag permits non-IAM actions on the tagged
  resource. Identity policies retain service-specific restrictions. Request tags
  alone do not satisfy this ownership check.
- Runtime grants outside those patterns and ownership checks retain their specific actions,
  resources and conditions, including grants that require `Resource: "*"`.
- Catalog grants selecting complete resource ARNs add those exact resources to
  the non-IAM ceiling. This includes encryption keys selected by container storage
  settings, even without a workload permission profile. KMS service and grant
  conditions remain in identity policies.
- Custom inline AWS permissions in workload profiles retain their actions,
  resources and conditions, compiled with their actual resource context.
  Management profiles do not contribute to the ceiling.

Provision, management, heartbeat, telemetry, observation, remote-management and
protection permission sets are excluded from the runtime catalog ceiling. A
boundary does not grant permissions by itself. Access granted by an identity
policy must also be allowed by the boundary. It is a deployment-wide ceiling for
those grants, not isolation between workloads. The default name-based ceiling
excludes IAM operations; custom inline grants must be reviewed for any broader access they add. Prefix
patterns can overlap with another deployment whose prefix begins with the same
prefix followed by a hyphen.

### Resource-policy grants

Resource policies also affect effective access. Within the same AWS account,
a grant to an IAM role ARN remains limited by the boundary. A grant directly to
an assumed-role session ARN can allow access omitted from the boundary. A
resource policy using `Principal: "*"` with an `aws:PrincipalArn` condition can
also grant access beyond those implicit limits. Applicable explicit denies
still override these grants. See AWS's [boundary evaluation rules](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies_boundaries.html)
and [role principal rules](https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_policies_elements_principal.html#principal-role).

The generated boundary therefore does not guarantee denial of all access outside
the deployment prefix. Resource owners must review their resource-policy grants
alongside workload identity policies. The boundary itself grants no authority
to change another resource's policy.

### Role writes and adoption

When setup has recorded a boundary ARN, generated management role-write grants
require `iam:PermissionsBoundary` to equal that ARN. Role creation supplies it;
policy and trust changes on existing roles first verify ownership and attach it
when missing. Adoption requires this deployment's runtime ownership tags.
Setup-owned roles, foreign roles, and roles with a different boundary are
refused. An unbounded role carrying an AWS-managed policy is also refused until
its permissions are reconciled in setup; a truncated attached-policy listing
fails rather than approving a partial audit.

Policy deletion and detachment normally use the same adoption check. Narrow
removal-only grants allow migration of the legacy DLM service policy and the
obsolete node SSM/CloudWatch policies; they do not allow attaching those policies.
`TagRole` and `DeleteRole` remain separately granted because those calls do not
supply the permissions-boundary condition key.

Management can still deploy code using permitted workload roles through
`iam:PassRole`. The ceiling constrains their identity-policy grants, but
management's own data-access denials do not transfer to them. Setup-only roles
retain their approved policy content. The boundary does not constrain trust
policy contents.

### Existing setups and generation limits

An absent recorded ARN means an older setup and preserves its existing behavior.
Deployments created before June 2026 should be recreated before rerunning setup.
Rerunning setup installs the boundary and records it. For a bounded deployment,
a deployment-time check compares the next release's required ceiling with the
recorded installed document. An expansion requires setup again before runtime
changes. With a recorded boundary, the management guard explicitly denies changes
to that policy's versions and tags, policy deletion, and removal of boundaries
from stack roles. Setup installer permissions are separate, so setup can update
and eventually delete the policy.

A restricted setup identity must include
`policy/${stackPrefix}-deployment-management-boundary` in its managed-policy
lifecycle permissions. An allowlist containing only the existing management
policy names will not cover this additional policy. This also applies to
`setupManagement: manual`; the boundary is not limited to automatic management.

IAM's managed-policy limit is 6,144 characters excluding whitespace. Generation
fails explicitly when the boundary exceeds it. CloudFormation and Terraform
budget maximum resolved input lengths, so a package can be rejected even when a
shorter concrete prefix would fit. Runtime grants selecting KMS keys use physical
key ARNs: templates reference key ARN attributes, and direct setup requires the
ARNs already present in imported state. A missing ARN is an error. Templates
include conditional catalog key grants only while the key is enabled, including
keys selected for native volume encryption. Inline grants referencing conditional
keys still require an unconditional key. Disabled keys contribute no ARN to the
installed or recorded boundary.

Teardown must remove runtime roles before setup deletes the boundary policy.
The setup registration dependencies preserve that ordering for generated
packages; the direct setup resource graph also orders roles before the policy.
Keep setup credentials available through privileged teardown; the management
identity cannot delete the boundary policy on the installer's behalf.
