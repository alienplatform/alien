# DaemonHeartbeatDataKubernetes

## Example Usage

```typescript
import { DaemonHeartbeatDataKubernetes } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataKubernetes = {
  commandSupported: false,
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
  backend: "kubernetes",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `commandSupported`                                                               | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `cpu`                                                                            | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `instances`                                                                      | [models.KubernetesPodInstanceStatus](../models/kubernetespodinstancestatus.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `replicas`                                                                       | [models.WorkloadReplicaStatus](../models/workloadreplicastatus.md)               | :heavy_check_mark:                                                               | N/A                                                                              |
| `restarts`                                                                       | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `workload`                                                                       | [models.KubernetesWorkloadStatus](../models/kubernetesworkloadstatus.md)         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetes"*                                                                   | :heavy_check_mark:                                                               | N/A                                                                              |