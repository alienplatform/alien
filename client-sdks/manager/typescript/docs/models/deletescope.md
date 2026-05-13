# DeleteScope

Scope for a delete operation.

Full deletes are setup/admin owned and may remove both Frozen and Live
resources. Live-only deletes are used by setup handoff resources
(Terraform/CloudFormation) so Alien removes only the resources it owns
before setup tears down Frozen resources.

## Example Usage

```typescript
import { DeleteScope } from "@alienplatform/manager-api/models";

let value: DeleteScope = "full";
```

## Values

```typescript
"full" | "liveOnly"
```