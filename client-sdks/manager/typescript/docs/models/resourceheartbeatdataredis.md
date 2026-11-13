# ResourceHeartbeatDataRedis

## Example Usage

```typescript
import { ResourceHeartbeatDataRedis } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataRedis = {
  data: {
    name: "<value>",
    processRunning: true,
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
      lifecycle: "deleting",
      partial: false,
      stale: false,
    },
    version: "<value>",
    backend: "local",
  },
  resourceType: "redis",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                                                          | *models.RedisHeartbeatData*                                                                                                                     | :heavy_check_mark:                                                                                                                              | Heartbeat data for a Redis resource. Everything comes from the cloud's<br/>management and metrics APIs; the manager never opens a Redis connection. |
| `resourceType`                                                                                                                                  | *"redis"*                                                                                                                                       | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
