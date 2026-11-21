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
      health: "healthy",
      lifecycle: "deleting",
      partial: true,
      stale: true,
    },
  },
  resourceType: "kubernetes-cluster",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                     | [operations.GetResourceDeploymentDetailData1](../../models/operations/getresourcedeploymentdetaildata1.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `resourceType`                                                                                             | *"kubernetes-cluster"*                                                                                     | :heavy_check_mark:                                                                                         | N/A                                                                                                        |