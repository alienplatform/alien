# DataKubernetes3

## Example Usage

```typescript
import { DataKubernetes3 } from "@alienplatform/platform-api/models/operations";

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
          controller: false,
          kind: "<value>",
          name: "<value>",
          uid: "<id>",
        },
      ],
      ready: true,
      restartCount: 402283,
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
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "kubernetes",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `commandSupported`                                                                                           | *boolean*                                                                                                    | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `cpu`                                                                                                        | *operations.CpuUnion6*                                                                                       | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `events`                                                                                                     | [operations.GetRawResourceHeartbeatEvent16](../../models/operations/getrawresourceheartbeatevent16.md)[]     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `instances`                                                                                                  | [operations.GetRawResourceHeartbeatInstance6](../../models/operations/getrawresourceheartbeatinstance6.md)[] | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `memory`                                                                                                     | *operations.MemoryUnion6*                                                                                    | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `name`                                                                                                       | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `namespace`                                                                                                  | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `replicas`                                                                                                   | [operations.Replicas4](../../models/operations/replicas4.md)                                                 | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `restarts`                                                                                                   | *number*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `status`                                                                                                     | [operations.DataStatus16](../../models/operations/datastatus16.md)                                           | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `workload`                                                                                                   | *operations.WorkloadUnion3*                                                                                  | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `backend`                                                                                                    | *"kubernetes"*                                                                                               | :heavy_check_mark:                                                                                           | N/A                                                                                                          |