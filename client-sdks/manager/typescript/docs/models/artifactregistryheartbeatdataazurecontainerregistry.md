# ArtifactRegistryHeartbeatDataAzureContainerRegistry

## Example Usage

```typescript
import { ArtifactRegistryHeartbeatDataAzureContainerRegistry } from "@alienplatform/manager-api/models";

let value: ArtifactRegistryHeartbeatDataAzureContainerRegistry = {
  adminUserEnabled: false,
  anonymousPullEnabled: true,
  dataEndpointHostNames: [],
  encryptionKeyIdentifierPresent: false,
  encryptionKeyVaultUriPresent: false,
  ipRuleCount: 615073,
  location: "<value>",
  managedTagCount: 61353,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: false,
  policyCount: 966370,
  privateEndpointConnectionCount: 211805,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: false,
  },
  zoneRedundancy: "<value>",
  backend: "azureContainerRegistry",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `adminUserEnabled`                                                                     | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `anonymousPullEnabled`                                                                 | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `creationDate`                                                                         | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `dataEndpointEnabled`                                                                  | *boolean*                                                                              | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `dataEndpointHostNames`                                                                | *string*[]                                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `encryptionKeyIdentifierPresent`                                                       | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `encryptionKeyVaultUriPresent`                                                         | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `encryptionStatus`                                                                     | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `ipRuleCount`                                                                          | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `location`                                                                             | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `loginServer`                                                                          | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `managedTagCount`                                                                      | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `name`                                                                                 | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `networkRuleBypassOptions`                                                             | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `networkRuleDefaultAction`                                                             | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `policiesPresent`                                                                      | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `policyCount`                                                                          | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `privateEndpointConnectionCount`                                                       | *number*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `provisioningState`                                                                    | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `publicNetworkAccess`                                                                  | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `resourceGroup`                                                                        | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `resourceId`                                                                           | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `skuName`                                                                              | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `skuTier`                                                                              | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.ArtifactRegistryHeartbeatStatus](../models/artifactregistryheartbeatstatus.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `type`                                                                                 | *string*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `zoneRedundancy`                                                                       | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"azureContainerRegistry"*                                                             | :heavy_check_mark:                                                                     | N/A                                                                                    |