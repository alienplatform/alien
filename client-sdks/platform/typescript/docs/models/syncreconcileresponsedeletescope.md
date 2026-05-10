# SyncReconcileResponseDeleteScope

Scope for a delete operation.

Full deletes are setup/admin owned and may remove both Frozen and Live
resources. Live-only deletes are used by setup handoff resources
(Terraform/CloudFormation) so Alien removes only the resources it owns
before setup tears down Frozen resources.

## Example Usage

```typescript
import { SyncReconcileResponseDeleteScope } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDeleteScope = "full";
```

## Values

```typescript
"full" | "liveOnly"
```