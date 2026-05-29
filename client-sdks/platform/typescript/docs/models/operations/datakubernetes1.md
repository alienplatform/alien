# DataKubernetes1

## Example Usage

```typescript
import { DataKubernetes1 } from "@alienplatform/platform-api/models/operations";

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

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `cpu`                                                                                                        | *operations.CpuUnion1*                                                                                       | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `events`                                                                                                     | [operations.GetRawResourceHeartbeatEvent8](../../models/operations/getrawresourceheartbeatevent8.md)[]       | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `instances`                                                                                                  | [operations.GetRawResourceHeartbeatInstance1](../../models/operations/getrawresourceheartbeatinstance1.md)[] | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `memory`                                                                                                     | *operations.MemoryUnion1*                                                                                    | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `name`                                                                                                       | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `namespace`                                                                                                  | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `replicas`                                                                                                   | [operations.Replicas1](../../models/operations/replicas1.md)                                                 | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `restarts`                                                                                                   | *number*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `status`                                                                                                     | [operations.DataStatus8](../../models/operations/datastatus8.md)                                             | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `triggerCount`                                                                                               | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `workload`                                                                                                   | *operations.WorkloadUnion1*                                                                                  | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `workloadKind`                                                                                               | [operations.WorkloadKind1](../../models/operations/workloadkind1.md)                                         | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `backend`                                                                                                    | *"kubernetes"*                                                                                               | :heavy_check_mark:                                                                                           | N/A                                                                                                          |