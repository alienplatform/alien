# ServiceAccountHeartbeatDataAzureManagedIdentity

## Example Usage

```typescript
import { ServiceAccountHeartbeatDataAzureManagedIdentity } from "@alienplatform/manager-api/models";

let value: ServiceAccountHeartbeatDataAzureManagedIdentity = {
  customRoleDefinitionCount: 128295,
  customRoleDefinitionIds: [
    "<value 1>",
    "<value 2>",
  ],
  events: [],
  location: "<value>",
  managedTagCount: 826820,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 359064,
  roleAssignmentIds: [
    "<value 1>",
  ],
  stackPermissionsApplied: true,
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
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `clientId`                                                                         | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `customRoleDefinitionCount`                                                        | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `customRoleDefinitionIds`                                                          | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `events`                                                                           | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `isolationScope`                                                                   | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `location`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `managedTagCount`                                                                  | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `principalId`                                                                      | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `resourceGroup`                                                                    | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `resourceId`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `roleAssignmentCount`                                                              | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `roleAssignmentIds`                                                                | *string*[]                                                                         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `stackPermissionsApplied`                                                          | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ServiceAccountHeartbeatStatus](../models/serviceaccountheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `tenantId`                                                                         | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `type`                                                                             | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"azureManagedIdentity"*                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |