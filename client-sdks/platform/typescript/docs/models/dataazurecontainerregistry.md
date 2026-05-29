# DataAzureContainerRegistry

## Example Usage

```typescript
import { DataAzureContainerRegistry } from "@alienplatform/platform-api/models";

let value: DataAzureContainerRegistry = {
  adminUserEnabled: false,
  anonymousPullEnabled: true,
  dataEndpointHostNames: [
    "<value 1>",
    "<value 2>",
  ],
  encryptionKeyIdentifierPresent: false,
  encryptionKeyVaultUriPresent: false,
  ipRuleCount: 806238,
  location: "<value>",
  managedTagCount: 395726,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: true,
  policyCount: 284832,
  privateEndpointConnectionCount: 849232,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  zoneRedundancy: "<value>",
  backend: "azureContainerRegistry",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `adminUserEnabled`                                         | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `anonymousPullEnabled`                                     | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `creationDate`                                             | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `dataEndpointEnabled`                                      | *boolean*                                                  | :heavy_minus_sign:                                         | N/A                                                        |
| `dataEndpointHostNames`                                    | *string*[]                                                 | :heavy_check_mark:                                         | N/A                                                        |
| `encryptionKeyIdentifierPresent`                           | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `encryptionKeyVaultUriPresent`                             | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `encryptionStatus`                                         | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `ipRuleCount`                                              | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `location`                                                 | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `loginServer`                                              | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `managedTagCount`                                          | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `name`                                                     | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `networkRuleBypassOptions`                                 | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `networkRuleDefaultAction`                                 | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `policiesPresent`                                          | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `policyCount`                                              | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `privateEndpointConnectionCount`                           | *number*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `provisioningState`                                        | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `publicNetworkAccess`                                      | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `resourceGroup`                                            | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `resourceId`                                               | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `skuName`                                                  | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `skuTier`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus48](../models/heartbeatstatus48.md) | :heavy_check_mark:                                         | N/A                                                        |
| `type`                                                     | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `zoneRedundancy`                                           | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"azureContainerRegistry"*                                 | :heavy_check_mark:                                         | N/A                                                        |