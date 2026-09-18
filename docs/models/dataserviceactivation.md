# DataServiceActivation

## Example Usage

```typescript
import { DataServiceActivation } from "@alienplatform/platform-api/models";

let value: DataServiceActivation = {
  data: {
    enabled: true,
    projectId: "<id>",
    serviceName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion15* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"service_activation"*                   | :heavy_check_mark:                       | N/A                                      |