# VaultHeartbeatDataAzureKeyVault

## Example Usage

```typescript
import { VaultHeartbeatDataAzureKeyVault } from "@alienplatform/manager-api/models";

let value: VaultHeartbeatDataAzureKeyVault = {
  accessPolicyCount: 590752,
  name: "<value>",
  privateEndpointConnectionCount: 62989,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: false,
  secretMetadataListed: true,
  softDeleteEnabled: true,
  softDeleteRetentionDays: 839995,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `accessPolicyCount`                                              | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `location`                                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `privateEndpointConnectionCount`                                 | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `provisioningState`                                              | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `publicNetworkAccess`                                            | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `purgeProtectionEnabled`                                         | *boolean*                                                        | :heavy_minus_sign:                                               | N/A                                                              |
| `rbacAuthorizationEnabled`                                       | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `resourceGroup`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `resourceId`                                                     | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `secretMetadataListed`                                           | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `skuFamily`                                                      | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `skuName`                                                        | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `softDeleteEnabled`                                              | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `softDeleteRetentionDays`                                        | *number*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.VaultHeartbeatStatus](../models/vaultheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `vaultUri`                                                       | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `backend`                                                        | *"azureKeyVault"*                                                | :heavy_check_mark:                                               | N/A                                                              |