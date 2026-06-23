# DataAzureKeyVault

## Example Usage

```typescript
import { DataAzureKeyVault } from "@alienplatform/platform-api/models";

let value: DataAzureKeyVault = {
  accessPolicyCount: 923246,
  name: "<value>",
  privateEndpointConnectionCount: 319306,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: true,
  secretMetadataListed: false,
  softDeleteEnabled: false,
  softDeleteRetentionDays: 497787,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azureKeyVault",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `accessPolicyCount`                                                        | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `privateEndpointConnectionCount`                                           | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `provisioningState`                                                        | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `publicNetworkAccess`                                                      | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `purgeProtectionEnabled`                                                   | *boolean*                                                                  | :heavy_minus_sign:                                                         | N/A                                                                        |
| `rbacAuthorizationEnabled`                                                 | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `secretMetadataListed`                                                     | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `skuFamily`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `skuName`                                                                  | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `softDeleteEnabled`                                                        | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `softDeleteRetentionDays`                                                  | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus33](../models/resourceheartbeatstatus33.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `vaultUri`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"azureKeyVault"*                                                          | :heavy_check_mark:                                                         | N/A                                                                        |