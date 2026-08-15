# ContainerRegistryManagerSnapshotRoute

## Example Usage

```typescript
import { ContainerRegistryManagerSnapshotRoute } from "@alienplatform/platform-api/models";

let value: ContainerRegistryManagerSnapshotRoute = {
  id: "crroute_c9t4xoy7fmiq7equtu20",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceRevision: "<value>",
  desiredRevision: 197709,
  appliedRevision: 106112,
  status: "resolving",
  repositories: [
    {
      id: "crrepo_625temdq3bnu25jw9rcux",
      logicalName: "<value>",
      desiredState: "deleteRequested",
      routableUpstreamName: "<value>",
      remoteResourceRevision: "<value>",
    },
  ],
  credentials: [],
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    | Example                                                                                                        |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `id`                                                                                                           | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Unique identifier for the container registry route.                                                            | crroute_c9t4xoy7fmiq7equtu20                                                                                   |
| `deploymentId`                                                                                                 | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `resourceId`                                                                                                   | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `resourceRevision`                                                                                             | *string*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `desiredRevision`                                                                                              | *number*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `appliedRevision`                                                                                              | *number*                                                                                                       | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `status`                                                                                                       | [models.ContainerRegistryManagerSnapshotStatus](../models/containerregistrymanagersnapshotstatus.md)           | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `repositories`                                                                                                 | [models.ContainerRegistryManagerSnapshotRepository](../models/containerregistrymanagersnapshotrepository.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |
| `credentials`                                                                                                  | [models.ContainerRegistryManagerSnapshotCredential](../models/containerregistrymanagersnapshotcredential.md)[] | :heavy_check_mark:                                                                                             | N/A                                                                                                            |                                                                                                                |