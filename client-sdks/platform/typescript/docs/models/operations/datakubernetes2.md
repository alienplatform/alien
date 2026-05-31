# DataKubernetes2

## Example Usage

```typescript
import { DataKubernetes2 } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetes2 = {
  events: [],
  name: "<value>",
  namespace: "<value>",
  pods: [],
  replicas: {},
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  workloadKind: "replicaSet",
  backend: "kubernetes",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `cpu`                                                                | *operations.CpuUnion4*                                               | :heavy_minus_sign:                                                   | N/A                                                                  |
| `events`                                                             | [operations.Event4](../../models/operations/event4.md)[]             | :heavy_check_mark:                                                   | N/A                                                                  |
| `memory`                                                             | *operations.MemoryUnion4*                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `namespace`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `pods`                                                               | [operations.Pod2](../../models/operations/pod2.md)[]                 | :heavy_check_mark:                                                   | N/A                                                                  |
| `replicas`                                                           | [operations.Replicas3](../../models/operations/replicas3.md)         | :heavy_check_mark:                                                   | N/A                                                                  |
| `restarts`                                                           | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [operations.DataStatus11](../../models/operations/datastatus11.md)   | :heavy_check_mark:                                                   | N/A                                                                  |
| `workload`                                                           | *operations.WorkloadUnion2*                                          | :heavy_minus_sign:                                                   | N/A                                                                  |
| `workloadKind`                                                       | [operations.WorkloadKind2](../../models/operations/workloadkind2.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `backend`                                                            | *"kubernetes"*                                                       | :heavy_check_mark:                                                   | N/A                                                                  |