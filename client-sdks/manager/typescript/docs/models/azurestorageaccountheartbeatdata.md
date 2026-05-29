# AzureStorageAccountHeartbeatData

## Example Usage

```typescript
import { AzureStorageAccountHeartbeatData } from "@alienplatform/manager-api/models";

let value: AzureStorageAccountHeartbeatData = {
  events: [],
  name: "<value>",
  primaryEndpoints: {},
  secondaryEndpoints: {},
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
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `allowBlobPublicAccess`                                                          | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `allowSharedKeyAccess`                                                           | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `encryptionKeySource`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `kind`                                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `minimumTlsVersion`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `networkBypass`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkDefaultAction`                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkIpRuleCount`                                                             | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkResourceAccessRuleCount`                                                 | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `networkVirtualNetworkRuleCount`                                                 | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `primaryEndpoints`                                                               | [models.AzureStorageAccountEndpoints](../models/azurestorageaccountendpoints.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `provisioningState`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `publicNetworkAccess`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `requireInfrastructureEncryption`                                                | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceGroup`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `resourceId`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `secondaryEndpoints`                                                             | [models.AzureStorageAccountEndpoints](../models/azurestorageaccountendpoints.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `skuName`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `skuTier`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.StorageHeartbeatStatus](../models/storageheartbeatstatus.md)             | :heavy_check_mark:                                                               | N/A                                                                              |
| `supportsHttpsTrafficOnly`                                                       | *boolean*                                                                        | :heavy_minus_sign:                                                               | N/A                                                                              |