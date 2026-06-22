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
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "deleted",
      partial: false,
      stale: false,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion14* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"service_activation"*                   | :heavy_check_mark:                       | N/A                                      |