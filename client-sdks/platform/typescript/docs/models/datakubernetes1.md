# DataKubernetes1

## Example Usage

```typescript
import { DataKubernetes1 } from "@alienplatform/platform-api/models";

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

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cpu`                                                                          | *models.CpuUnion1*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent1](../models/syncreconcilerequestevent1.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion1*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `namespace`                                                                    | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `pods`                                                                         | [models.Pod1](../models/pod1.md)[]                                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | [models.Replicas1](../models/replicas1.md)                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `restarts`                                                                     | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus8](../models/heartbeatstatus8.md)                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `triggerCount`                                                                 | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `workload`                                                                     | *models.WorkloadUnion1*                                                        | :heavy_minus_sign:                                                             | N/A                                                                            |
| `workloadKind`                                                                 | [models.WorkloadKind1](../models/workloadkind1.md)                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"kubernetes"*                                                                 | :heavy_check_mark:                                                             | N/A                                                                            |