# DataKubernetes1

## Example Usage

```typescript
import { DataKubernetes1 } from "@alienplatform/platform-api/models";

let value: DataKubernetes1 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-15T13:58:09.990Z"),
      severity: "info",
    },
  ],
  instances: [
    {
      name: "<value>",
      ownerReferences: [
        {
          controller: false,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 303382,
    },
  ],
  name: "<value>",
  namespace: "<value>",
  replicas: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  triggerCount: 638946,
  workloadKind: "replicaSet",
  backend: "kubernetes",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `cpu`                                                                          | *models.CpuUnion1*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent8](../models/syncreconcilerequestevent8.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `instances`                                                                    | [models.Instance1](../models/instance1.md)[]                                   | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion1*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `namespace`                                                                    | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | [models.Replicas1](../models/replicas1.md)                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `restarts`                                                                     | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus8](../models/heartbeatstatus8.md)                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `triggerCount`                                                                 | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `workload`                                                                     | *models.WorkloadUnion1*                                                        | :heavy_minus_sign:                                                             | N/A                                                                            |
| `workloadKind`                                                                 | [models.WorkloadKind1](../models/workloadkind1.md)                             | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"kubernetes"*                                                                 | :heavy_check_mark:                                                             | N/A                                                                            |