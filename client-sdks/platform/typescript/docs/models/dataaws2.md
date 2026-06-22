# DataAws2

## Example Usage

```typescript
import { DataAws2 } from "@alienplatform/platform-api/models";

let value: DataAws2 = {
  capacityGroups: [],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "aws",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `backendClusterId`                                                         | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `capacityGroups`                                                           | [models.CapacityGroup1](../models/capacitygroup1.md)[]                     | :heavy_check_mark:                                                         | N/A                                                                        |
| `cpu`                                                                      | *models.CpuUnion7*                                                         | :heavy_minus_sign:                                                         | N/A                                                                        |
| `memory`                                                                   | *models.MemoryUnion7*                                                      | :heavy_minus_sign:                                                         | N/A                                                                        |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `nodes`                                                                    | [models.Nodes1](../models/nodes1.md)                                       | :heavy_check_mark:                                                         | N/A                                                                        |
| `providerFleets`                                                           | [models.ProviderFleet1](../models/providerfleet1.md)[]                     | :heavy_check_mark:                                                         | N/A                                                                        |
| `region`                                                                   | *string*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus18](../models/resourceheartbeatstatus18.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"aws"*                                                                    | :heavy_check_mark:                                                         | N/A                                                                        |