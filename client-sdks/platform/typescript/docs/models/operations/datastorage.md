# DataStorage

## Example Usage

```typescript
import { DataStorage } from "@alienplatform/platform-api/models/operations";

let value: DataStorage = {
  data: {
    path: "/home",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "scaling",
      partial: false,
      stale: false,
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