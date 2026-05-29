# DataStorage

## Example Usage

```typescript
import { DataStorage } from "@alienplatform/platform-api/models/operations";

let value: DataStorage = {
  data: {
    events: [],
    path: "/var/mail",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "deleting",
      partial: true,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion1* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"storage"*             | :heavy_check_mark:      | N/A                     |