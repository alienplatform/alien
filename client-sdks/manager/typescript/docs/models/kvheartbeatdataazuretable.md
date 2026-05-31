# KvHeartbeatDataAzureTable

## Example Usage

```typescript
import { KvHeartbeatDataAzureTable } from "@alienplatform/manager-api/models";

let value: KvHeartbeatDataAzureTable = {
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
    lifecycle: "creating",
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

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `endpoint`                                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `resourceGroup`                                            | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `signedIdentifierCount`                                    | *number*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.KvHeartbeatStatus](../models/kvheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `storageAccountKind`                                       | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `storageAccountLocation`                                   | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `storageAccountName`                                       | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `storageAccountPrimaryStatus`                              | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `storageAccountProvisioningState`                          | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `storageAccountResourceId`                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `tableExists`                                              | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `tableName`                                                | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"azureTable"*                                             | :heavy_check_mark:                                         | N/A                                                        |