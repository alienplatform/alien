# DataComputeCluster

## Example Usage

```typescript
import { DataComputeCluster } from "@alienplatform/platform-api/models/operations";

let value: DataComputeCluster = {
  data: {
    dockerAvailable: true,
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
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    backend: "local",
  },
  resourceType: "compute-cluster",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion5* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"compute-cluster"*     | :heavy_check_mark:      | N/A                     |