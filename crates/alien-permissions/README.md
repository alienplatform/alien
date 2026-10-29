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

AWS setup creates `${stackPrefix}-permissions-boundary` alongside the management
identity's policies. Direct setup, CloudFormation and Terraform use
`aws_stack_permissions_boundary` and record both the installed policy ARN and
its document. Setup owns creation, updates and deletion of this managed policy.
Management's generated grants do not permit editing it or removing a role's
permissions boundary.

The document defines a ceiling for roles management writes:

- Name-based runtime catalog bindings contribute stack-prefixed ARN patterns.
  Resource-specific names widen to the deployment prefix. One `Allow` statement
  permits actions other than `iam:*` on these patterns.
- Runtime grants outside those patterns retain their specific actions,
  resources and conditions, including grants that require `Resource: "*"`.
- Custom inline AWS permissions in workload profiles and catalog grants that
  select complete resource ARNs are compiled with their actual resource context.
  These are setup-approved additions to the ceiling; they are not restricted
  to name-based resources. Management profiles do not contribute to it.

Native volume-key grants derived from container storage settings are not yet
included unless they also appear in a workload profile. Encrypted node and
backup policies therefore need further integration before using this ceiling.

Provision, management, heartbeat, telemetry, observation, remote-management and
protection permission sets are excluded from the runtime catalog ceiling. A
boundary does not grant permissions by itself: the role still needs an identity
policy that allows the operation. It is a deployment-wide ceiling, not isolation
between workloads. The default name-based ceiling excludes IAM operations;
custom inline grants must be reviewed for any broader access they add. Prefix
patterns can overlap with another deployment whose prefix begins with the same
prefix followed by a hyphen.

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
`iam:PassRole`. The ceiling constrains those roles' effective permissions, but
management's own data-access denials do not transfer to them. Setup-only roles
retain their approved policy content. The boundary does not constrain trust
policy contents.

### Existing setups and generation limits

An absent recorded ARN means an older setup and preserves its existing behavior.
Rerunning setup installs the boundary and records it. For a bounded deployment,
a deployment-time check compares the next release's required ceiling with the
recorded installed document. An expansion requires setup again before runtime
changes; management does not rewrite the boundary itself.

IAM's managed-policy limit is 6,144 characters excluding whitespace. Generation
fails explicitly when the boundary exceeds it. CloudFormation and Terraform
budget maximum resolved input lengths, so a package can be rejected even when a
shorter concrete prefix would fit. Runtime grants selecting KMS keys use physical
key ARNs: templates reference key ARN attributes, and direct setup requires the
ARNs already present in imported state. A missing ARN is an error. Templates
reject conditional keys selected by runtime boundary grants; unused conditional
keys remain supported.

Teardown must remove runtime roles before setup deletes the boundary policy.
The setup registration dependencies preserve that ordering for generated
packages; the direct setup resource graph also orders roles before the policy.
