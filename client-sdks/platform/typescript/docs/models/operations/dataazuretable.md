# DataAzureTable

## Example Usage

```typescript
import { DataAzureTable } from "@alienplatform/platform-api/models/operations";

let value: DataAzureTable = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-08T10:49:40.534Z"),
      severity: "warning",
    },
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  storageAccountName: "<value>",
  tableExists: false,
  tableName: "<value>",
  backend: "azureTable",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `endpoint`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent29](../../models/operations/getrawresourceheartbeatevent29.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `resourceGroup`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `signedIdentifierCount`                                                                                  | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus29](../../models/operations/datastatus29.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `storageAccountKind`                                                                                     | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `storageAccountLocation`                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `storageAccountName`                                                                                     | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `storageAccountPrimaryStatus`                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `storageAccountProvisioningState`                                                                        | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `storageAccountResourceId`                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `tableExists`                                                                                            | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `tableName`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"azureTable"*                                                                                           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |