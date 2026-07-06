# ResourceHeartbeatDataComputeCluster

## Example Usage

```typescript
import { ResourceHeartbeatDataComputeCluster } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataComputeCluster = {
  data: {
    capacityGroups: [],
    machines: [
      {
        capacityGroup: "<value>",
        drainForce: true,
        lastHeartbeat: "<value>",
        machineId: "<id>",
        replicaCount: 818208,
        status: "<value>",
        zone: "<value>",
      },
    ],
    name: "<value>",
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
      health: "unhealthy",
      lifecycle: "creating",
      partial: true,
      stale: false,
    },
    backend: "machines",
  },
  resourceType: "compute-cluster",
};
```

## Fields

| Field                                | Type                                 | Required                             | Description                          |
| ------------------------------------ | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `data`                               | *models.ComputeClusterHeartbeatData* | :heavy_check_mark:                   | N/A                                  |
| `resourceType`                       | *"compute-cluster"*                  | :heavy_check_mark:                   | N/A                                  |