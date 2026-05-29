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
  events: [],
  name: "<value>",
  nodes: {},
  providerFleets: [
    {
      currentMachines: 22022,
      desiredMachines: 972056,
      groupId: "<id>",
      providerId: "<id>",
    },
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
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
| `events`                                                                         | [models.SyncReconcileRequestEvent19](../models/syncreconcilerequestevent19.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion8*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodes`                                                                          | [models.Nodes2](../models/nodes2.md)                                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `providerFleets`                                                                 | [models.ProviderFleet2](../models/providerfleet2.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus19](../models/heartbeatstatus19.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcp"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |