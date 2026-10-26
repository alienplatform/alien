# SyncListResponseDeletionPolicy

Explicit disposition of durable data when deleting a deployment.
Missing values from older checkpoints always preserve data.

## Example Usage

```typescript
import { SyncListResponseDeletionPolicy } from "@alienplatform/platform-api/models";

let value: SyncListResponseDeletionPolicy = "erase";
```

## Values

```typescript
"retain" | "erase"
```