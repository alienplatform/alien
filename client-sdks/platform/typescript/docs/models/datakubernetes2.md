# DataKubernetes2

## Example Usage

```typescript
import { DataKubernetes2 } from "@alienplatform/platform-api/models";

let value: DataKubernetes2 = {
  events: [],
  instances: [],
  name: "<value>",
  namespace: "<value>",
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

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | *models.CpuUnion4*                                                               | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent11](../models/syncreconcilerequestevent11.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `instances`                                                                      | [models.Instance2](../models/instance2.md)[]                                     | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion4*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `replicas`                                                                       | [models.Replicas3](../models/replicas3.md)                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `restarts`                                                                       | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus11](../models/heartbeatstatus11.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `workload`                                                                       | *models.WorkloadUnion2*                                                          | :heavy_minus_sign:                                                               | N/A                                                                              |
| `workloadKind`                                                                   | [models.WorkloadKind2](../models/workloadkind2.md)                               | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetes"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |