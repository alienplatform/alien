# DataKubernetes3

## Example Usage

```typescript
import { DataKubernetes3 } from "@alienplatform/platform-api/models";

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

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `cpu`                                                                          | *models.CpuUnion6*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent9](../models/syncreconcilerequestevent9.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion6*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `namespace`                                                                    | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `pods`                                                                         | [models.Pod3](../models/pod3.md)[]                                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | [models.Replicas4](../models/replicas4.md)                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `restarts`                                                                     | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus16](../models/heartbeatstatus16.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `workload`                                                                     | *models.WorkloadUnion3*                                                        | :heavy_minus_sign:                                                             | N/A                                                                            |
| `backend`                                                                      | *"kubernetes"*                                                                 | :heavy_check_mark:                                                             | N/A                                                                            |