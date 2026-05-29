# DataAzureTable

## Example Usage

```typescript
import { DataAzureTable } from "@alienplatform/platform-api/models";

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
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
  storageAccountName: "<value>",
  tableExists: true,
  tableName: "<value>",
  backend: "azureTable",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `endpoint`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent29](../models/syncreconcilerequestevent29.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `signedIdentifierCount`                                                          | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus29](../models/heartbeatstatus29.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `storageAccountKind`                                                             | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `storageAccountLocation`                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `storageAccountName`                                                             | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `storageAccountPrimaryStatus`                                                    | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `storageAccountProvisioningState`                                                | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `storageAccountResourceId`                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `tableExists`                                                                    | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `tableName`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"azureTable"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |