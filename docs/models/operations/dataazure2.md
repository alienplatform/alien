# DataAzure2

## Example Usage

```typescript
import { DataAzure2 } from "@alienplatform/platform-api/models/operations";

let value: DataAzure2 = {
  capacityGroups: [
    {
      currentMachines: 986352,
      desiredMachines: 1134,
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
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleting",
    partial: true,
    stale: true,
  },
  backend: "azure",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `backendClusterId`                                                       | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `capacityGroups`                                                         | [operations.CapacityGroup3](../../models/operations/capacitygroup3.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `cpu`                                                                    | *operations.CpuUnion9*                                                   | :heavy_minus_sign:                                                       | N/A                                                                      |
| `memory`                                                                 | *operations.MemoryUnion9*                                                | :heavy_minus_sign:                                                       | N/A                                                                      |
| `name`                                                                   | *string*                                                                 | :heavy_check_mark:                                                       | N/A                                                                      |
| `nodes`                                                                  | [operations.Nodes3](../../models/operations/nodes3.md)                   | :heavy_check_mark:                                                       | N/A                                                                      |
| `providerFleets`                                                         | [operations.ProviderFleet3](../../models/operations/providerfleet3.md)[] | :heavy_check_mark:                                                       | N/A                                                                      |
| `region`                                                                 | *string*                                                                 | :heavy_minus_sign:                                                       | N/A                                                                      |
| `status`                                                                 | [operations.DataStatus21](../../models/operations/datastatus21.md)       | :heavy_check_mark:                                                       | N/A                                                                      |
| `backend`                                                                | *"azure"*                                                                | :heavy_check_mark:                                                       | N/A                                                                      |