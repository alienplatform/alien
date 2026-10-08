# ResolvedCommandTargetDeliveryMode

How a command is delivered to its target resource.

This is a Commands-protocol-specific concept and is intentionally distinct
from `DeploymentModel` (see `stack_settings.rs`), which governs the
infrastructure-level push/pull wiring for a deployment. Serialized
lowercase for consistency with `CommandTargetType`.

## Example Usage

```typescript
import { ResolvedCommandTargetDeliveryMode } from "@alienplatform/platform-api/models";

let value: ResolvedCommandTargetDeliveryMode = "push";
```

## Values

```typescript
"push" | "pull"
```