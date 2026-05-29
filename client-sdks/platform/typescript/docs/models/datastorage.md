# DataStorage

## Example Usage

```typescript
import { DataStorage } from "@alienplatform/platform-api/models";

let value: DataStorage = {
  data: {
    events: [],
    path: "/var/mail",
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
      health: "healthy",
      lifecycle: "stopping",
      partial: false,
      stale: false,
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