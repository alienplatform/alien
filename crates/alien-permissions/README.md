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
