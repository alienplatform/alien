# ResourceHeartbeatDataComputeCluster

## Example Usage

```typescript
import { ResourceHeartbeatDataComputeCluster } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataComputeCluster = {
  data: {
    dockerAvailable: true,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    networkAvailable: true,
    nodes: {},
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
      lifecycle: "deleted",
      partial: true,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "compute-cluster",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `data`                               | *models.ComputeClusterHeartbeatData* | :heavy_check_mark:                   | N/A                                  |
| `resourceType`                       | *"compute-cluster"*                  | :heavy_check_mark:                   | N/A                                  |