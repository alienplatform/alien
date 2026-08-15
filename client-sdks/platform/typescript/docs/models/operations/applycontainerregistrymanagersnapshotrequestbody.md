# ApplyContainerRegistryManagerSnapshotRequestBody

## Example Usage

```typescript
import { ApplyContainerRegistryManagerSnapshotRequestBody } from "@alienplatform/platform-api/models/operations";

let value: ApplyContainerRegistryManagerSnapshotRequestBody = {
  routeId: "crroute_c9t4xoy7fmiq7equtu20",
  desiredRevision: 103893,
  repositories: [],
  verification: {
    succeeded: true,
    observedAt: new Date("2024-01-04T07:56:27.719Z"),
    error: "<value>",
  },
};
```

## Fields

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                | Example                                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `routeId`                                                                                                                                  | *string*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Unique identifier for the container registry route.                                                                                        | crroute_c9t4xoy7fmiq7equtu20                                                                                                               |
| `desiredRevision`                                                                                                                          | *number*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |                                                                                                                                            |
| `repositories`                                                                                                                             | [operations.ApplyContainerRegistryManagerSnapshotRepository](../../models/operations/applycontainerregistrymanagersnapshotrepository.md)[] | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |                                                                                                                                            |
| `verification`                                                                                                                             | [operations.Verification](../../models/operations/verification.md)                                                                         | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |                                                                                                                                            |