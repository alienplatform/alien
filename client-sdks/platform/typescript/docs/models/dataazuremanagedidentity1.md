# DataAzureManagedIdentity1

## Example Usage

```typescript
import { DataAzureManagedIdentity1 } from "@alienplatform/platform-api/models";

let value: DataAzureManagedIdentity1 = {
  customRoleDefinitionCount: 863031,
  customRoleDefinitionIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  location: "<value>",
  managedTagCount: 765674,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 833585,
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  stackPermissionsApplied: false,
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
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `clientId`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `customRoleDefinitionCount`                                                | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `customRoleDefinitionIds`                                                  | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `isolationScope`                                                           | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `location`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `managedTagCount`                                                          | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `principalId`                                                              | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `resourceGroup`                                                            | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceId`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `roleAssignmentCount`                                                      | *number*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `roleAssignmentIds`                                                        | *string*[]                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `stackPermissionsApplied`                                                  | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus38](../models/resourceheartbeatstatus38.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `tenantId`                                                                 | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `type`                                                                     | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `backend`                                                                  | *"azureManagedIdentity"*                                                   | :heavy_check_mark:                                                         | N/A                                                                        |