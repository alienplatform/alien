# ApplyContainerRegistryManagerSnapshotRepository

## Example Usage

```typescript
import { ApplyContainerRegistryManagerSnapshotRepository } from "@alienplatform/platform-api/models/operations";

let value: ApplyContainerRegistryManagerSnapshotRepository = {
  id: "crrepo_625temdq3bnu25jw9rcux",
  status: "ready",
  routableUpstreamName: "<value>",
  error: "<value>",
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      | Example                                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                                             | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Unique identifier for the container registry repository.                                                                         | crrepo_625temdq3bnu25jw9rcux                                                                                                     |
| `status`                                                                                                                         | [operations.ApplyContainerRegistryManagerSnapshotStatus](../../models/operations/applycontainerregistrymanagersnapshotstatus.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |                                                                                                                                  |
| `routableUpstreamName`                                                                                                           | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |                                                                                                                                  |
| `error`                                                                                                                          | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |                                                                                                                                  |