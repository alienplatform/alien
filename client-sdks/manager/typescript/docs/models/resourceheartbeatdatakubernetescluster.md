# ResourceHeartbeatDataKubernetesCluster

## Example Usage

```typescript
import { ResourceHeartbeatDataKubernetesCluster } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataKubernetesCluster = {
  data: {
    events: [],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "deleted",
      partial: false,
      stale: true,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `data`                                                                               | [models.KubernetesClusterHeartbeatData](../models/kubernetesclusterheartbeatdata.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `resourceType`                                                                       | *"kubernetes-cluster"*                                                               | :heavy_check_mark:                                                                   | N/A                                                                                  |