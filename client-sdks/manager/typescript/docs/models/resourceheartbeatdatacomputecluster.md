# ResourceHeartbeatDataComputeCluster

## Example Usage

```typescript
import { ResourceHeartbeatDataComputeCluster } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataComputeCluster = {
  data: {
    dockerAvailable: true,
    name: "<value>",
    networkAvailable: false,
    nodes: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
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