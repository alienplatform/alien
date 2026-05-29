# WorkerHeartbeatDataKubernetes

## Example Usage

```typescript
import { WorkerHeartbeatDataKubernetes } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataKubernetes = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  instances: [],
  name: "<value>",
  namespace: "<value>",
  replicas: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 374516,
  workloadKind: "daemonSet",
  backend: "kubernetes",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `instances`                                                                      | [models.KubernetesPodInstanceStatus](../models/kubernetespodinstancestatus.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `replicas`                                                                       | [models.WorkloadReplicaStatus](../models/workloadreplicastatus.md)               | :heavy_check_mark:                                                               | N/A                                                                              |
| `restarts`                                                                       | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `triggerCount`                                                                   | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `workload`                                                                       | [models.KubernetesWorkloadStatus](../models/kubernetesworkloadstatus.md)         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `workloadKind`                                                                   | [models.KubernetesWorkloadKind](../models/kubernetesworkloadkind.md)             | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetes"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |