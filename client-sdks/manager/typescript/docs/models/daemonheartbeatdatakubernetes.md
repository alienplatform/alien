# DaemonHeartbeatDataKubernetes

## Example Usage

```typescript
import { DaemonHeartbeatDataKubernetes } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataKubernetes = {
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
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "kubernetes",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `commandSupported`                                                                     | *boolean*                                                                              | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `cpu`                                                                                  | [models.MetricSample](../models/metricsample.md)                                       | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `events`                                                                               | [models.KubernetesEventSnapshot](../models/kuberneteseventsnapshot.md)[]               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `memory`                                                                               | [models.MetricSample](../models/metricsample.md)                                       | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `name`                                                                                 | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `namespace`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `pods`                                                                                 | [models.KubernetesPodRuntimeUnitStatus](../models/kubernetespodruntimeunitstatus.md)[] | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `replicas`                                                                             | [models.WorkloadReplicaStatus](../models/workloadreplicastatus.md)                     | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `restarts`                                                                             | *number*                                                                               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `status`                                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)                 | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `workload`                                                                             | [models.KubernetesWorkloadStatus](../models/kubernetesworkloadstatus.md)               | :heavy_minus_sign:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"kubernetes"*                                                                         | :heavy_check_mark:                                                                     | N/A                                                                                    |