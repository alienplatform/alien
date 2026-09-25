# alien-permissions

Permission system for Alien. Compiles permission sets from JSONC definitions, evaluates policies, and interpolates cloud-specific IAM variables.

Permission sets are defined in `permission-sets/` as JSONC files, specifying the cloud IAM permissions required for each resource type. Split into management (provisioning) and application (runtime) scopes.

For application queues, use `queue/publish` for producers and `queue/data-read`
for consumers. The consumer set includes acknowledgement and visibility
operations as well as receive. `queue/data-write` remains available for
existing profiles that need its combined send-and-acknowledge access.
An unqualified queue link still receives the older combined grants by default;
use an explicit resource profile to keep a publisher or consumer one-way.

## Core Types

- `PermissionContext` — Builder for cloud permission variable context (AWS account/region, GCP project, Azure subscription, etc.)
- `VariableInterpolator` — Cross-cloud permission variable interpolation
- `get_permission_set()` / `list_permission_set_ids()` — Permission set registry operations
