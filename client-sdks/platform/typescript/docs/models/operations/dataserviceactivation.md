# DataServiceActivation

## Example Usage

```typescript
import { DataServiceActivation } from "@alienplatform/platform-api/models/operations";

let value: DataServiceActivation = {
  data: {
    enabled: true,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-06-11T19:58:11.111Z"),
        severity: "error",
      },
    ],
    projectId: "<id>",
    serviceName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "stopping",
      partial: false,
      stale: true,
    },
    backend: "gcpServiceUsage",
  },
  resourceType: "service_activation",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *operations.DataUnion14* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"service_activation"*   | :heavy_check_mark:       | N/A                      |