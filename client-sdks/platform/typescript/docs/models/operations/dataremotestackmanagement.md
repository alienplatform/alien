# DataRemoteStackManagement

## Example Usage

```typescript
import { DataRemoteStackManagement } from "@alienplatform/platform-api/models/operations";

let value: DataRemoteStackManagement = {
  data: {
    events: [],
    managementPermissionsApplied: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
    backend: "awsIamRole",
  },
  resourceType: "remote-stack-management",
};
```

## Fields

| Field                       | Type                        | Required                    | Description                 |
| --------------------------- | --------------------------- | --------------------------- | --------------------------- |
| `data`                      | *operations.DataUnion11*    | :heavy_check_mark:          | N/A                         |
| `resourceType`              | *"remote-stack-management"* | :heavy_check_mark:          | N/A                         |