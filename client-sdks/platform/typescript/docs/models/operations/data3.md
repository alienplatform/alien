# Data3

## Example Usage

```typescript
import { Data3 } from "@alienplatform/platform-api/models/operations";

let value: Data3 = {
  name: "<value>",
  primaryEndpoints: {},
  secondaryEndpoints: {},
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `allowBlobPublicAccess`                                                        | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `allowSharedKeyAccess`                                                         | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `encryptionKeySource`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `kind`                                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `location`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `minimumTlsVersion`                                                            | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `networkBypass`                                                                | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `networkDefaultAction`                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `networkIpRuleCount`                                                           | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `networkResourceAccessRuleCount`                                               | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `networkVirtualNetworkRuleCount`                                               | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `primaryEndpoints`                                                             | [operations.PrimaryEndpoints](../../models/operations/primaryendpoints.md)     | :heavy_check_mark:                                                             | N/A                                                                            |
| `provisioningState`                                                            | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `publicNetworkAccess`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `requireInfrastructureEncryption`                                              | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `resourceGroup`                                                                | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `resourceId`                                                                   | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `secondaryEndpoints`                                                           | [operations.SecondaryEndpoints](../../models/operations/secondaryendpoints.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `skuName`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `skuTier`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [operations.DataStatus57](../../models/operations/datastatus57.md)             | :heavy_check_mark:                                                             | N/A                                                                            |
| `supportsHttpsTrafficOnly`                                                     | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |