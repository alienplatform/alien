# DataAzureKeyVault

## Example Usage

```typescript
import { DataAzureKeyVault } from "@alienplatform/platform-api/models/operations";

let value: DataAzureKeyVault = {
  accessPolicyCount: 923246,
  events: [],
  name: "<value>",
  privateEndpointConnectionCount: 363497,
  publicNetworkAccess: "<value>",
  rbacAuthorizationEnabled: false,
  secretMetadataListed: false,
  softDeleteEnabled: true,
  softDeleteRetentionDays: 24015,
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
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "azureKeyVault",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `accessPolicyCount`                                                                                      | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent33](../../models/operations/getrawresourceheartbeatevent33.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `location`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `privateEndpointConnectionCount`                                                                         | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `provisioningState`                                                                                      | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `publicNetworkAccess`                                                                                    | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `purgeProtectionEnabled`                                                                                 | *boolean*                                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `rbacAuthorizationEnabled`                                                                               | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `resourceGroup`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `resourceId`                                                                                             | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `secretMetadataListed`                                                                                   | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `skuFamily`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `skuName`                                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `softDeleteEnabled`                                                                                      | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `softDeleteRetentionDays`                                                                                | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus33](../../models/operations/datastatus33.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `vaultUri`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"azureKeyVault"*                                                                                        | :heavy_check_mark:                                                                                       | N/A                                                                                                      |