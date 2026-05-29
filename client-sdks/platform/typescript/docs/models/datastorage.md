# DataStorage

## Example Usage

```typescript
import { DataStorage } from "@alienplatform/platform-api/models";

let value: DataStorage = {
  data: {
    path: "/home",
    pathExists: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "stopping",
      partial: false,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion1* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"storage"*                             | :heavy_check_mark:                      | N/A                                     |