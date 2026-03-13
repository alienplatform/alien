# GetContainerOverviewTotals

## Example Usage

```typescript
import { GetContainerOverviewTotals } from "@aliendotdev/platform-api/models/operations";

let value: GetContainerOverviewTotals = {
  deployments: 106451,
  containerInstances: 551069,
  machines: 239785,
  machinesByStatus: {
    running: 439065,
    unhealthy: 394151,
    initializing: 457741,
    draining: 712653,
  },
  reschedulingFrozenCount: 341567,
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `deployments`                                                                                                      | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | Total deployments with containers                                                                                  |
| `containerInstances`                                                                                               | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `machines`                                                                                                         | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `machinesByStatus`                                                                                                 | [operations.GetContainerOverviewMachinesByStatus](../../models/operations/getcontaineroverviewmachinesbystatus.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `reschedulingFrozenCount`                                                                                          | *number*                                                                                                           | :heavy_check_mark:                                                                                                 | Clusters where rescheduling is frozen                                                                              |