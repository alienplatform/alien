# DeploymentDeletionPolicy

Explicit disposition of durable data when deleting a deployment.
Missing values from older checkpoints always preserve data.

## Example Usage

```typescript
import { DeploymentDeletionPolicy } from "@alienplatform/platform-api/models";

let value: DeploymentDeletionPolicy = "erase";
```

## Values

```typescript
"retain" | "erase"
```