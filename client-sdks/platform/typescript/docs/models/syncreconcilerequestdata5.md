# SyncReconcileRequestData5

## Example Usage

```typescript
import { SyncReconcileRequestData5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData5 = {
  events: [],
  name: "<value>",
  privateEndpointConnectionCount: 333052,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `createdAt`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `disableLocalAuth`                                                               | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent59](../models/syncreconcilerequestevent59.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `metricId`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `minimumTlsVersion`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespaceStatus`                                                                | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `premiumMessagingPartitions`                                                     | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `privateEndpointConnectionCount`                                                 | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `provisioningState`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicNetworkAccess`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `serviceBusEndpoint`                                                             | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `skuCapacity`                                                                    | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `skuName`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `skuTier`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus59](../models/heartbeatstatus59.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `updatedAt`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `zoneRedundant`                                                                  | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |