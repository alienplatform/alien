# DataMachines2

## Example Usage

```typescript
import { DataMachines2 } from "@alienplatform/platform-api/models/operations";

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
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "machines",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `backendClusterId`                                                       | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `capacityGroups`                                                         | [operations.CapacityGroup4](../../models/operations/capacitygroup4.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `cpu`                                                                    | *operations.CpuUnion10*                                                  | :heavy_minus_sign:                                                       | N/A                                                                      |
| `machines`                                                               | [operations.Machine](../../models/operations/machine.md)[]               | :heavy_check_mark:                                                       | N/A                                                                      |
| `memory`                                                                 | *operations.MemoryUnion10*                                               | :heavy_minus_sign:                                                       | N/A                                                                      |
| `name`                                                                   | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `nodes`                                                                  | [operations.Nodes4](../../models/operations/nodes4.md)                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `status`                                                                 | [operations.DataStatus22](../../models/operations/datastatus22.md)       | :heavy_check_mark:                                                       | N/A                                                                      |
| `backend`                                                                | *"machines"*                                                             | :heavy_check_mark:                                                       | N/A                                                                      |