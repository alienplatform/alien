# ResourceHeartbeatDataRemoteStackManagement

## Example Usage

```typescript
import { ResourceHeartbeatDataRemoteStackManagement } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataRemoteStackManagement = {
  data: {
    events: [],
    managementPermissionsApplied: true,
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
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

## Fields

| Field                                       | Type                                        | Required                                    | Description                                 |
| ------------------------------------------- | ------------------------------------------- | ------------------------------------------- | ------------------------------------------- |
| `data`                                      | *models.RemoteStackManagementHeartbeatData* | :heavy_check_mark:                          | N/A                                         |
| `resourceType`                              | *"remote-stack-management"*                 | :heavy_check_mark:                          | N/A                                         |