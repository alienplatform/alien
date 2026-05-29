# ResourceHeartbeatDataStorage

## Example Usage

```typescript
import { ResourceHeartbeatDataStorage } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataStorage = {
  data: {
    events: [],
    path: "/dev",
    pathExists: true,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: true,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "storage",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `data`                        | *models.StorageHeartbeatData* | :heavy_check_mark:            | N/A                           |
| `resourceType`                | *"storage"*                   | :heavy_check_mark:            | N/A                           |