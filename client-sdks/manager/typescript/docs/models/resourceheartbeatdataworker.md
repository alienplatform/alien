# ResourceHeartbeatDataWorker

## Example Usage

```typescript
import { ResourceHeartbeatDataWorker } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataWorker = {
  data: {
    appName: "<value>",
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
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