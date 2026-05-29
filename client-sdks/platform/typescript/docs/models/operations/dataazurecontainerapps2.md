# DataAzureContainerApps2

## Example Usage

```typescript
import { DataAzureContainerApps2 } from "@alienplatform/platform-api/models/operations";

let value: DataAzureContainerApps2 = {
  environmentVariableCount: 246098,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-12-26T22:26:53.665Z"),
      severity: "info",
    },
  ],
  managedEnvironmentId: "<id>",
  resourceGroupName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  backend: "azureContainerApps",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `environmentVariableCount`                                                                               | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent52](../../models/operations/getrawresourceheartbeatevent52.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `managedEnvironmentId`                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `managedIdentityId`                                                                                      | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `resourceGroupName`                                                                                      | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `resourcePrefix`                                                                                         | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus52](../../models/operations/datastatus52.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"azureContainerApps"*                                                                                   | :heavy_check_mark:                                                                                       | N/A                                                                                                      |