# ContainerRegistryManagerSnapshotRepository

## Example Usage

```typescript
import { ContainerRegistryManagerSnapshotRepository } from "@alienplatform/platform-api/models";

let value: ContainerRegistryManagerSnapshotRepository = {
  id: "crrepo_625temdq3bnu25jw9rcux",
  logicalName: "<value>",
  desiredState: "deleted",
  routableUpstreamName: "<value>",
  remoteResourceRevision: "<value>",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      | Example                                                                                                          |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                             | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Unique identifier for the container registry repository.                                                         | crrepo_625temdq3bnu25jw9rcux                                                                                     |
| `logicalName`                                                                                                    | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |                                                                                                                  |
| `desiredState`                                                                                                   | [models.ContainerRegistryManagerSnapshotDesiredState](../models/containerregistrymanagersnapshotdesiredstate.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |                                                                                                                  |
| `routableUpstreamName`                                                                                           | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |                                                                                                                  |
| `remoteResourceRevision`                                                                                         | *string*                                                                                                         | :heavy_check_mark:                                                                                               | N/A                                                                                                              |                                                                                                                  |