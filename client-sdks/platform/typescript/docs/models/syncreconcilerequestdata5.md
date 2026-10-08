# SyncReconcileRequestData5

## Example Usage

```typescript
import { SyncReconcileRequestData5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData5 = {
  name: "<value>",
  privateEndpointConnectionCount: 396107,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `createdAt`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `disableLocalAuth`                                                         | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `metricId`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `minimumTlsVersion`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `namespaceStatus`                                                          | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `premiumMessagingPartitions`                                               | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `privateEndpointConnectionCount`                                           | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `provisioningState`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `publicNetworkAccess`                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `serviceBusEndpoint`                                                       | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `skuCapacity`                                                              | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `skuName`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `skuTier`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus65](../models/resourceheartbeatstatus65.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `updatedAt`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `zoneRedundant`                                                            | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |