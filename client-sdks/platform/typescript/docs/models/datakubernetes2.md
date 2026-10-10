# DataKubernetes2

## Example Usage

```typescript
import { DataKubernetes2 } from "@alienplatform/platform-api/models";

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

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cpu`                                                                          | *models.CpuUnion4*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent4](../models/syncreconcilerequestevent4.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion4*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `namespace`                                                                    | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `pods`                                                                         | [models.Pod2](../models/pod2.md)[]                                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | [models.Replicas3](../models/replicas3.md)                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `restarts`                                                                     | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.ResourceHeartbeatStatus11](../models/resourceheartbeatstatus11.md)     | :heavy_check_mark:                                                             | N/A                                                                            |
| `workload`                                                                     | *models.WorkloadUnion2*                                                        | :heavy_minus_sign:                                                             | N/A                                                                            |
| `workloadKind`                                                                 | [models.WorkloadKind2](../models/workloadkind2.md)                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"kubernetes"*                                                                 | :heavy_check_mark:                                                             | N/A                                                                            |