# DataAws2

## Example Usage

```typescript
import { DataAws2 } from "@alienplatform/platform-api/models/operations";

let value: DataAws2 = {
  capacityGroups: [],
  events: [],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "aws",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `backendClusterId`                                                                                       | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `capacityGroups`                                                                                         | [operations.CapacityGroup1](../../models/operations/capacitygroup1.md)[]                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `cpu`                                                                                                    | *operations.CpuUnion7*                                                                                   | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent18](../../models/operations/getrawresourceheartbeatevent18.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `memory`                                                                                                 | *operations.MemoryUnion7*                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `nodes`                                                                                                  | [operations.Nodes1](../../models/operations/nodes1.md)                                                   | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `providerFleets`                                                                                         | [operations.ProviderFleet1](../../models/operations/providerfleet1.md)[]                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `region`                                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus18](../../models/operations/datastatus18.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"aws"*                                                                                                  | :heavy_check_mark:                                                                                       | N/A                                                                                                      |