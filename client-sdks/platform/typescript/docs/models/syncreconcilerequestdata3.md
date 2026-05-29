# SyncReconcileRequestData3

## Example Usage

```typescript
import { SyncReconcileRequestData3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData3 = {
  events: [],
  name: "<value>",
  primaryEndpoints: {},
  secondaryEndpoints: {},
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
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `allowBlobPublicAccess`                                                          | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `allowSharedKeyAccess`                                                           | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `encryptionKeySource`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent57](../models/syncreconcilerequestevent57.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `kind`                                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `minimumTlsVersion`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `networkBypass`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkDefaultAction`                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkIpRuleCount`                                                             | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkResourceAccessRuleCount`                                                 | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkVirtualNetworkRuleCount`                                                 | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `primaryEndpoints`                                                               | [models.PrimaryEndpoints](../models/primaryendpoints.md)                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `provisioningState`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicNetworkAccess`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `requireInfrastructureEncryption`                                                | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `secondaryEndpoints`                                                             | [models.SecondaryEndpoints](../models/secondaryendpoints.md)                     | :heavy_check_mark:                                                               | N/A                                                                              |
| `skuName`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `skuTier`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus57](../models/heartbeatstatus57.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `supportsHttpsTrafficOnly`                                                       | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |