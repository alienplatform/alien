# ContainerHeartbeatDataKubernetes

## Example Usage

```typescript
import { ContainerHeartbeatDataKubernetes } from "@alienplatform/manager-api/models";

let value: ContainerHeartbeatDataKubernetes = {
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
  workloadKind: "deployment",
  backend: "kubernetes",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
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
| `workloadKind`                                                                         | [models.KubernetesWorkloadKind](../models/kubernetesworkloadkind.md)                   | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `backend`                                                                              | *"kubernetes"*                                                                         | :heavy_check_mark:                                                                     | N/A                                                                                    |