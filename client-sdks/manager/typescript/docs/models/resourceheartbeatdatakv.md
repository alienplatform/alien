# ResourceHeartbeatDataKv

## Example Usage

```typescript
import { ResourceHeartbeatDataKv } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataKv = {
  data: {
    keySchema: [],
    name: "<value>",
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
      lifecycle: "creating",
      partial: false,
      stale: false,
    },
    backend: "awsDynamoDb",
  },
  resourceType: "kv",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *models.KvHeartbeatData* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"kv"*                   | :heavy_check_mark:       | N/A                      |