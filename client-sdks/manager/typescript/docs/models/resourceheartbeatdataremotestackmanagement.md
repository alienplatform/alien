# ResourceHeartbeatDataRemoteStackManagement

## Example Usage

```typescript
import { ResourceHeartbeatDataRemoteStackManagement } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataRemoteStackManagement = {
  data: {
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "running",
      partial: true,
      stale: true,
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