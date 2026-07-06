# DataServiceAccount

## Example Usage

```typescript
import { DataServiceAccount } from "@alienplatform/platform-api/models";

let value: DataServiceAccount = {
  data: {
    customRoleDefinitionCount: 991371,
    customRoleDefinitionIds: [
      "<value 1>",
      "<value 2>",
      "<value 3>",
    ],
    location: "<value>",
    managedTagCount: 438151,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 703891,
    roleAssignmentIds: [
      "<value 1>",
    ],
    stackPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
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

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion10* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"service-account"*                      | :heavy_check_mark:                       | N/A                                      |