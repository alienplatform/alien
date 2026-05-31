# DataRemoteStackManagement

## Example Usage

```typescript
import { DataRemoteStackManagement } from "@alienplatform/platform-api/models";

let value: DataRemoteStackManagement = {
  data: {
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "stopping",
      partial: true,
      stale: false,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion11* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"remote-stack-management"*              | :heavy_check_mark:                       | N/A                                      |