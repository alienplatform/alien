# AzureServiceBusNamespaceHeartbeatData

## Example Usage

```typescript
import { AzureServiceBusNamespaceHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureServiceBusNamespaceHeartbeatData = {
  events: [],
  name: "<value>",
  privateEndpointConnectionCount: 992899,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `createdAt`                                                      | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `disableLocalAuth`                                               | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `location`                                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `metricId`                                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `minimumTlsVersion`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `namespaceStatus`                                                | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `premiumMessagingPartitions`                                     | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `privateEndpointConnectionCount`                                 | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `provisioningState`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `publicNetworkAccess`                                            | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `resourceGroup`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `resourceId`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `serviceBusEndpoint`                                             | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `skuCapacity`                                                    | *number*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `skuName`                                                        | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `skuTier`                                                        | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.QueueHeartbeatStatus](../models/queueheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `updatedAt`                                                      | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `zoneRedundant`                                                  | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |