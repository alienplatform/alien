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
    events: [],
    location: "<value>",
    managedTagCount: 703891,
    name: "<value>",
    resourceGroup: "<value>",
    resourceId: "<id>",
    roleAssignmentCount: 447985,
    roleAssignmentIds: [],
    stackPermissionsApplied: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    backend: "azureManagedIdentity",
  },
  resourceType: "service-account",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion9* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"service-account"*                     | :heavy_check_mark:                      | N/A                                     |