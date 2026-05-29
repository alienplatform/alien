# DataAws2

## Example Usage

```typescript
import { DataAws2 } from "@alienplatform/platform-api/models";

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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "aws",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `backendClusterId`                                                               | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `capacityGroups`                                                                 | [models.CapacityGroup1](../models/capacitygroup1.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `cpu`                                                                            | *models.CpuUnion7*                                                               | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent18](../models/syncreconcilerequestevent18.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion7*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodes`                                                                          | [models.Nodes1](../models/nodes1.md)                                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `providerFleets`                                                                 | [models.ProviderFleet1](../models/providerfleet1.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus18](../models/heartbeatstatus18.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"aws"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |