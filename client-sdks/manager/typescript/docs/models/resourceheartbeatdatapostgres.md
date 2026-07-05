# ResourceHeartbeatDataPostgres

## Example Usage

```typescript
import { ResourceHeartbeatDataPostgres } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataPostgres = {
  data: {
    instanceName: "<value>",
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "deleting",
      partial: false,
      stale: false,
    },
    backend: "cloudSql",
  },
  resourceType: "postgres",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `data`                         | *models.PostgresHeartbeatData* | :heavy_check_mark:             | N/A                            |
| `resourceType`                 | *"postgres"*                   | :heavy_check_mark:             | N/A                            |