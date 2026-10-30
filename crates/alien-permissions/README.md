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

Direct setup, CloudFormation and Terraform create
`${stackPrefix}-deployment-management-boundary` and record its ARN and document.
Setup owns the policy lifecycle. Management cannot edit it or remove role boundaries.
Without a recorded ARN, deployments retain their existing behavior; rerunning setup
installs the boundary. Deployments created before June 2026 should be recreated
before rerunning setup. Later releases needing a wider ceiling require setup again
before runtime changes.

The generated ceiling permits non-IAM actions on runtime catalog prefix patterns,
resources with existing deployment ownership tags, and selected physical ARNs
(including volume encryption keys). Other runtime grants and custom inline workload
grants retain their actions, resources and conditions. Management, provision,
heartbeat, telemetry, observation and protection sets do not contribute. Custom
grants can extend the ceiling and require setup review. The boundary grants no
access itself and does not isolate workloads within a deployment; prefix patterns
can also overlap when one prefix starts with another followed by a hyphen.

Role-write grants require `iam:PermissionsBoundary` to equal the recorded ARN.
[`IamRoleWriter`](../alien-aws-clients/src/aws/iam/role_writer.rs) supplies it at
creation and attaches it before existing-role writes. Adoption requires runtime
ownership tags and rejects foreign/setup-owned roles, different boundaries,
incomplete policy inventories and unbounded roles carrying AWS-managed policies.
Ordinary policy cleanup uses the same check. Narrow removal-only grants cover
obsolete DLM and node SSM/CloudWatch policies; they do not permit reattachment.
`TagRole` and `DeleteRole` have separate grants. Setup-only roles remain setup-owned.

Management can deploy code with permitted roles through `iam:PassRole`; its own
data-access denials do not follow those roles. The ceiling does not constrain trust
policy contents or guarantee denial of all access outside the prefix. Same-account
resource policies granting directly to sessions, or using a wildcard principal
with `aws:PrincipalArn`, can bypass implicit boundary limits; explicit denies still
apply. Review resource policies separately, including any workload permissions to
change them. See AWS's [boundary evaluation rules](https://docs.aws.amazon.com/IAM/latest/UserGuide/access_policies_boundaries.html).

Generation fails above IAM's 6,144-character managed-policy limit, excluding
whitespace. Templates budget maximum input lengths, so shorter actual inputs may
fit a rejected template. KMS grants require physical key ARNs; missing ARNs fail.
Conditional catalog keys contribute only when enabled; inline grants require an
unconditional key.

Restricted installers must allow policy lifecycle operations on
`policy/${stackPrefix}-deployment-management-boundary`, including for manual setup.
Keep installer credentials through teardown: remove runtime roles first, then
let setup delete the boundary. Management cannot delete it for the installer.
