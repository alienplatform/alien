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
    events: [],
    location: "<value>",
    managedTagCount: 127599,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 916453,
    roleAssignmentIds: [],
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
      health: "degraded",
      lifecycle: "deleting",
      partial: true,
      stale: false,
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