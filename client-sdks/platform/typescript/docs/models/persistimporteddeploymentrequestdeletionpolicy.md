# PersistImportedDeploymentRequestDeletionPolicy

Explicit disposition of durable data when deleting a deployment.
Missing values from older checkpoints always preserve data.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestDeletionPolicy } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestDeletionPolicy = "retain";
```

## Values

```typescript
"retain" | "erase"
```