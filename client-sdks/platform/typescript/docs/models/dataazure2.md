# DataAzure2

## Example Usage

```typescript
import { DataAzure2 } from "@alienplatform/platform-api/models";

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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
  backend: "azure",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `backendClusterId`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `capacityGroups`                                                           | [models.CapacityGroup3](../models/capacitygroup3.md)[]                     | :heavy_check_mark:                                                         | N/A                                                                        |
| `cpu`                                                                      | *models.CpuUnion9*                                                         | :heavy_minus_sign:                                                         | N/A                                                                        |
| `memory`                                                                   | *models.MemoryUnion9*                                                      | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `nodes`                                                                    | [models.Nodes3](../models/nodes3.md)                                       | :heavy_check_mark:                                                         | N/A                                                                        |
| `providerFleets`                                                           | [models.ProviderFleet3](../models/providerfleet3.md)[]                     | :heavy_check_mark:                                                         | N/A                                                                        |
| `region`                                                                   | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus20](../models/resourceheartbeatstatus20.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"azure"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |