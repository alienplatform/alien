# ResourceHeartbeatDataWorker

## Example Usage

```typescript
import { ResourceHeartbeatDataWorker } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataWorker = {
  data: {
    appName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    backend: "azureContainerApps",
  },
  resourceType: "worker",
};
```

## Fields

| Field                        | Type                         | Required                     | Description                  |
| ---------------------------- | ---------------------------- | ---------------------------- | ---------------------------- |
| `data`                       | *models.WorkerHeartbeatData* | :heavy_check_mark:           | N/A                          |
| `resourceType`               | *"worker"*                   | :heavy_check_mark:           | N/A                          |