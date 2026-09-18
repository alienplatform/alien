# DataGcp2

## Example Usage

```typescript
import { DataGcp2 } from "@alienplatform/platform-api/models";

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
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleted",
    partial: false,
    stale: true,
  },
  backend: "gcp",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `backendClusterId`                                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `capacityGroups`                                                                 | [models.CapacityGroup2](../models/capacitygroup2.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `cpu`                                                                            | *models.CpuUnion8*                                                               | :heavy_minus_sign:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion8*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodes`                                                                          | [models.Nodes2](../models/nodes2.md)                                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `providerFleets`                                                                 | [models.ProviderFleet2](../models/providerfleet2.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SyncReconcileRequestStatus20](../models/syncreconcilerequeststatus20.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcp"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |