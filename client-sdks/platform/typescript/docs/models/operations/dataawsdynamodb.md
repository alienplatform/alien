# DataAwsDynamoDb

## Example Usage

```typescript
import { DataAwsDynamoDb } from "@alienplatform/platform-api/models/operations";

let value: DataAwsDynamoDb = {
  keySchema: [
    {
      attributeName: "<value>",
      keyType: "<value>",
    },
  ],
  name: "<value>",
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
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  backend: "awsDynamoDb",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `billingMode`                                                      | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `deletionProtectionEnabled`                                        | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `globalSecondaryIndexCount`                                        | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `itemCount`                                                        | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `keySchema`                                                        | [operations.KeySchema](../../models/operations/keyschema.md)[]     | :heavy_check_mark:                                                 | N/A                                                                |
| `localSecondaryIndexCount`                                         | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `region`                                                           | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `replicaCount`                                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `restoreInProgress`                                                | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `sseStatus`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `sseType`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus27](../../models/operations/datastatus27.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `streamEnabled`                                                    | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `streamViewType`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `tableArn`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `tableClass`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `tableSizeBytes`                                                   | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `tableStatus`                                                      | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `ttlAttributeName`                                                 | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `ttlStatus`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"awsDynamoDb"*                                                    | :heavy_check_mark:                                                 | N/A                                                                |