# ComputeDrainProgress

## Example Usage

```typescript
import { ComputeDrainProgress } from "@alienplatform/manager-api/models";

let value: ComputeDrainProgress = {
  force: true,
  machineId: "<id>",
  replicaCount: 878999,
  stalled: false,
  status: "draining",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `blockers`                                                                   | [models.ComputeDrainBlocker](../models/computedrainblocker.md)[]             | :heavy_minus_sign:                                                           | N/A                                                                          |
| `drainDeadlineAt`                                                            | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `drainRequestedAt`                                                           | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `drainedAt`                                                                  | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `force`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `machineId`                                                                  | *string*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `replicaCount`                                                               | *number*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `stalled`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `status`                                                                     | [models.ComputeDrainProgressStatus](../models/computedrainprogressstatus.md) | :heavy_check_mark:                                                           | N/A                                                                          |