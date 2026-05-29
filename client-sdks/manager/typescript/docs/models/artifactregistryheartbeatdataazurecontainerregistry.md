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
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  ipRuleCount: 61353,
  location: "<value>",
  managedTagCount: 809501,
  name: "<value>",
  networkRuleBypassOptions: "<value>",
  policiesPresent: false,
  policyCount: 211805,
  privateEndpointConnectionCount: 509149,
  publicNetworkAccess: "<value>",
  resourceGroup: "<value>",
  skuName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopped",
    partial: false,
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
| `events`                                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                                 | :heavy_check_mark:                                                                     | N/A                                                                                    |
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