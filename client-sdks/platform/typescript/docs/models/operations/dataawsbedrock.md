# DataAwsBedrock

## Example Usage

```typescript
import { DataAwsBedrock } from "@alienplatform/platform-api/models/operations";

let value: DataAwsBedrock = {
  availability: {
    catalogRevision: "<value>",
    models: [],
    source: "aws-bedrock",
  },
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "awsBedrock",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `availability`                                                                                                           | [operations.Availability1](../../models/operations/availability1.md)                                                     | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `region`                                                                                                                 | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus66](../../models/operations/getresourcedeploymentdetaildatastatus66.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `backend`                                                                                                                | *"awsBedrock"*                                                                                                           | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |