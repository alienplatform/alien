# DataRedis

## Example Usage

```typescript
import { DataRedis } from "@alienplatform/platform-api/models";

let value: DataRedis = {
  data: {
    config: {},
    instanceName: "<value>",
    metrics: {},
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
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    backend: "memorystore",
  },
  resourceType: "redis",
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                                                          | *models.SyncReconcileRequestDataUnion9*                                                                                                         | :heavy_check_mark:                                                                                                                              | Heartbeat data for a Redis resource. Everything comes from the cloud's<br/>management and metrics APIs; the manager never opens a Redis connection. |
| `resourceType`                                                                                                                                  | [models.ResourceTypeRedis](../models/resourcetyperedis.md)                                                                                      | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |