# DeploymentStateDeletionPolicy

Explicit disposition of durable data when deleting a deployment.
Missing values from older checkpoints always preserve data.

## Example Usage

```typescript
import { DeploymentStateDeletionPolicy } from "@alienplatform/platform-api/models";

let value: DeploymentStateDeletionPolicy = "retain";
```

## Values

```typescript
"retain" | "erase"
```