# ResourceHeartbeatDataStorage

## Example Usage

```typescript
import { ResourceHeartbeatDataStorage } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataStorage = {
  data: {
    path: "/Library",
    pathExists: true,
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "stopping",
      partial: true,
      stale: true,
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