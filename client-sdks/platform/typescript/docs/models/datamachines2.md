# DataMachines2

## Example Usage

```typescript
import { DataMachines2 } from "@alienplatform/platform-api/models";

let value: DataMachines2 = {
  capacityGroups: [
    {
      currentMachines: 818927,
      desiredMachines: 925900,
      groupId: "<id>",
    },
  ],
  machines: [],
  name: "<value>",
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  backend: "machines",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `backendClusterId`                                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `capacityGroups`                                                                 | [models.CapacityGroup4](../models/capacitygroup4.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `cpu`                                                                            | *models.CpuUnion10*                                                              | :heavy_minus_sign:                                                               | N/A                                                                              |
| `machines`                                                                       | [models.SyncReconcileRequestMachine](../models/syncreconcilerequestmachine.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion10*                                                           | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodes`                                                                          | [models.Nodes4](../models/nodes4.md)                                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.ResourceHeartbeatStatus22](../models/resourceheartbeatstatus22.md)       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"machines"*                                                                     | :heavy_check_mark:                                                               | N/A                                                                              |