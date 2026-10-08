# ResourceHeartbeatDataServiceAccount

## Example Usage

```typescript
import { ResourceHeartbeatDataServiceAccount } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataServiceAccount = {
  data: {
    customRoleDefinitionCount: 783312,
    customRoleDefinitionIds: [
      "<value 1>",
    ],
    location: "<value>",
    managedTagCount: 22826,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 127599,
    roleAssignmentIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
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
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
      stale: true,
    },
    backend: "azureManagedIdentity",
  },
  resourceType: "service-account",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `data`                               | *models.ServiceAccountHeartbeatData* | :heavy_check_mark:                   | N/A                                  |
| `resourceType`                       | *"service-account"*                  | :heavy_check_mark:                   | N/A                                  |