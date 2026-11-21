# DataLocal9

Local Valkey container.

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal9 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  version: "<value>",
  backend: "local",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `name`                                                                                                                   | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `port`                                                                                                                   | *number*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `processRunning`                                                                                                         | *boolean*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus40](../../models/operations/getresourcedeploymentdetaildatastatus40.md) | :heavy_check_mark:                                                                                                       | Same fields as every other resource heartbeat status.                                                                    |
| `version`                                                                                                                | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `backend`                                                                                                                | *"local"*                                                                                                                | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |