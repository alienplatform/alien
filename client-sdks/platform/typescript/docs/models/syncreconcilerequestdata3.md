# SyncReconcileRequestData3

## Example Usage

```typescript
import { SyncReconcileRequestData3 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData3 = {
  name: "<value>",
  primaryEndpoints: {},
  secondaryEndpoints: {},
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `allowBlobPublicAccess`                                                    | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `allowSharedKeyAccess`                                                     | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `encryptionKeySource`                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `kind`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `minimumTlsVersion`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `networkBypass`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `networkDefaultAction`                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `networkIpRuleCount`                                                       | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `networkResourceAccessRuleCount`                                           | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `networkVirtualNetworkRuleCount`                                           | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `primaryEndpoints`                                                         | [models.PrimaryEndpoints](../models/primaryendpoints.md)                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `provisioningState`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `publicNetworkAccess`                                                      | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `requireInfrastructureEncryption`                                          | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `secondaryEndpoints`                                                       | [models.SecondaryEndpoints](../models/secondaryendpoints.md)               | :heavy_check_mark:                                                         | N/A                                                                        |
| `skuName`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `skuTier`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus57](../models/resourceheartbeatstatus57.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `supportsHttpsTrafficOnly`                                                 | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |