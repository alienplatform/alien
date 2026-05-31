# Data5

## Example Usage

```typescript
import { Data5 } from "@alienplatform/platform-api/models/operations";

let value: Data5 = {
  name: "<value>",
  privateEndpointConnectionCount: 460668,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `createdAt`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `disableLocalAuth`                                                 | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `location`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `metricId`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `minimumTlsVersion`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `namespaceStatus`                                                  | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `premiumMessagingPartitions`                                       | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `privateEndpointConnectionCount`                                   | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `provisioningState`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `publicNetworkAccess`                                              | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceGroup`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceId`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `serviceBusEndpoint`                                               | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `skuCapacity`                                                      | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `skuName`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `skuTier`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus59](../../models/operations/datastatus59.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `updatedAt`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `zoneRedundant`                                                    | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |