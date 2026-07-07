# DataGcp2

## Example Usage

```typescript
import { DataGcp2 } from "@alienplatform/platform-api/models/operations";

let value: DataGcp2 = {
  capacityGroups: [
    {
      currentMachines: 496999,
      desiredMachines: 708581,
      groupId: "<id>",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "failed",
    partial: true,
    stale: false,
  },
  backend: "gcp",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `backendClusterId`                                                       | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `capacityGroups`                                                         | [operations.CapacityGroup2](../../models/operations/capacitygroup2.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `cpu`                                                                    | *operations.CpuUnion8*                                                   | :heavy_minus_sign:                                                       | N/A                                                                      |
| `memory`                                                                 | *operations.MemoryUnion8*                                                | :heavy_minus_sign:                                                       | N/A                                                                      |
| `name`                                                                   | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `nodes`                                                                  | [operations.Nodes2](../../models/operations/nodes2.md)                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `providerFleets`                                                         | [operations.ProviderFleet2](../../models/operations/providerfleet2.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `region`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `status`                                                                 | [operations.DataStatus20](../../models/operations/datastatus20.md)       | :heavy_check_mark:                                                       | N/A                                                                      |
| `backend`                                                                | *"gcp"*                                                                  | :heavy_check_mark:                                                       | N/A                                                                      |