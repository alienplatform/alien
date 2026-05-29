# DataKubernetesCluster

## Example Usage

```typescript
import { DataKubernetesCluster } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetesCluster = {
  data: {
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data1](../../models/operations/data1.md) | :heavy_check_mark:                                   | N/A                                                  |
| `resourceType`                                       | *"kubernetes-cluster"*                               | :heavy_check_mark:                                   | N/A                                                  |