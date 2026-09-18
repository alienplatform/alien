# DataAzureTable

## Example Usage

```typescript
import { DataAzureTable } from "@alienplatform/platform-api/models/operations";

let value: DataAzureTable = {
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "scaling",
    partial: false,
    stale: true,
  },
  storageAccountName: "<value>",
  tableExists: true,
  tableName: "<value>",
  backend: "azureTable",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `endpoint`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceGroup`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `signedIdentifierCount`                                            | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus31](../../models/operations/datastatus31.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `storageAccountKind`                                               | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `storageAccountLocation`                                           | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `storageAccountName`                                               | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `storageAccountPrimaryStatus`                                      | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `storageAccountProvisioningState`                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `storageAccountResourceId`                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `tableExists`                                                      | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `tableName`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"azureTable"*                                                     | :heavy_check_mark:                                                 | N/A                                                                |