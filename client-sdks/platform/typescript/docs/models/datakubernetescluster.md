# DataKubernetesCluster

## Example Usage

```typescript
import { DataKubernetesCluster } from "@alienplatform/platform-api/models";

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
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "stopping",
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