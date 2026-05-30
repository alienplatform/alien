# DataKubernetes3

## Example Usage

```typescript
import { DataKubernetes3 } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetes3 = {
  commandSupported: false,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  name: "<value>",
  namespace: "<value>",
  pods: [],
  replicas: {},
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "kubernetes",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `commandSupported`                                                 | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `cpu`                                                              | *operations.CpuUnion6*                                             | :heavy_minus_sign:                                                 | N/A                                                                |
| `events`                                                           | [operations.Event9](../../models/operations/event9.md)[]           | :heavy_check_mark:                                                 | N/A                                                                |
| `memory`                                                           | *operations.MemoryUnion6*                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `namespace`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `pods`                                                             | [operations.Pod3](../../models/operations/pod3.md)[]               | :heavy_check_mark:                                                 | N/A                                                                |
| `replicas`                                                         | [operations.Replicas4](../../models/operations/replicas4.md)       | :heavy_check_mark:                                                 | N/A                                                                |
| `restarts`                                                         | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus16](../../models/operations/datastatus16.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `workload`                                                         | *operations.WorkloadUnion3*                                        | :heavy_minus_sign:                                                 | N/A                                                                |
| `backend`                                                          | *"kubernetes"*                                                     | :heavy_check_mark:                                                 | N/A                                                                |