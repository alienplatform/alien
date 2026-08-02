# ResourceHeartbeatDataAi

## Example Usage

```typescript
import { ResourceHeartbeatDataAi } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAi = {
  data: {
    accountName: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "creating",
      partial: false,
      stale: true,
    },
    backend: "azureFoundry",
  },
  resourceType: "ai",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *models.AiHeartbeatData* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"ai"*                   | :heavy_check_mark:       | N/A                      |