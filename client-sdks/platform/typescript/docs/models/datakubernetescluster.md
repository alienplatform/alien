# DataKubernetesCluster

## Example Usage

```typescript
import { DataKubernetesCluster } from "@alienplatform/platform-api/models";

let value: DataKubernetesCluster = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2026-06-21T07:51:22.353Z"),
        severity: "info",
      },
    ],
    name: "<value>",
    nodeCounts: {},
    podCounts: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData1](../models/syncreconcilerequestdata1.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceType`                                                             | *"kubernetes-cluster"*                                                     | :heavy_check_mark:                                                         | N/A                                                                        |