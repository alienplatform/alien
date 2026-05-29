# DataComputeCluster

## Example Usage

```typescript
import { DataComputeCluster } from "@alienplatform/platform-api/models";

let value: DataComputeCluster = {
  data: {
    dockerAvailable: true,
    events: [],
    name: "<value>",
    networkAvailable: true,
    nodes: {},
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "compute-cluster",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion5* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"compute-cluster"*                     | :heavy_check_mark:                      | N/A                                     |