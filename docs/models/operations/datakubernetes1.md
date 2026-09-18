# DataKubernetes1

## Example Usage

```typescript
import { DataKubernetes1 } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetes1 = {
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
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  triggerCount: 303382,
  workloadKind: "daemonSet",
  backend: "kubernetes",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `cpu`                                                                | *operations.CpuUnion1*                                               | :heavy_minus_sign:                                                   | N/A                                                                  |
| `events`                                                             | [operations.Event1](../../models/operations/event1.md)[]             | :heavy_check_mark:                                                   | N/A                                                                  |
| `memory`                                                             | *operations.MemoryUnion1*                                            | :heavy_minus_sign:                                                   | N/A                                                                  |
| `name`                                                               | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `namespace`                                                          | *string*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `pods`                                                               | [operations.Pod1](../../models/operations/pod1.md)[]                 | :heavy_check_mark:                                                   | N/A                                                                  |
| `replicas`                                                           | [operations.Replicas1](../../models/operations/replicas1.md)         | :heavy_check_mark:                                                   | N/A                                                                  |
| `restarts`                                                           | *number*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `status`                                                             | [operations.DataStatus8](../../models/operations/datastatus8.md)     | :heavy_check_mark:                                                   | N/A                                                                  |
| `triggerCount`                                                       | *number*                                                             | :heavy_check_mark:                                                   | N/A                                                                  |
| `workload`                                                           | *operations.WorkloadUnion1*                                          | :heavy_minus_sign:                                                   | N/A                                                                  |
| `workloadKind`                                                       | [operations.WorkloadKind1](../../models/operations/workloadkind1.md) | :heavy_check_mark:                                                   | N/A                                                                  |
| `backend`                                                            | *"kubernetes"*                                                       | :heavy_check_mark:                                                   | N/A                                                                  |