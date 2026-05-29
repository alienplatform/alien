# DataKubernetesCluster

## Example Usage

```typescript
import { DataKubernetesCluster } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetesCluster = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-08-02T12:07:39.617Z"),
        severity: "error",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
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