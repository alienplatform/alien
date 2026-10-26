# DeploymentDetailResponseDeletionPolicy

Explicit disposition of durable data when deleting a deployment.
Missing values from older checkpoints always preserve data.

## Example Usage

```typescript
import { DeploymentDetailResponseDeletionPolicy } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseDeletionPolicy = "retain";
```

## Values

```typescript
"retain" | "erase"
```