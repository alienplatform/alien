# DataKubernetes3

## Example Usage

```typescript
import { DataKubernetes3 } from "@alienplatform/platform-api/models";

let value: DataKubernetes3 = {
  commandSupported: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-29T04:54:57.889Z"),
      severity: "warning",
    },
  ],
  instances: [
    {
      name: "<value>",
      ownerReferences: [
        {
          controller: true,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 905674,
    },
  ],
  name: "<value>",
  namespace: "<value>",
  replicas: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "kubernetes",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `commandSupported`                                                               | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `cpu`                                                                            | *models.CpuUnion6*                                                               | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent16](../models/syncreconcilerequestevent16.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `instances`                                                                      | [models.Instance6](../models/instance6.md)[]                                     | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion6*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `replicas`                                                                       | [models.Replicas4](../models/replicas4.md)                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `restarts`                                                                       | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus16](../models/heartbeatstatus16.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `workload`                                                                       | *models.WorkloadUnion3*                                                          | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetes"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |