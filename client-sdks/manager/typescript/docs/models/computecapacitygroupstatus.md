# ComputeCapacityGroupStatus

## Example Usage

```typescript
import { ComputeCapacityGroupStatus } from "@alienplatform/manager-api/models";

let value: ComputeCapacityGroupStatus = {
  currentMachines: 511996,
  desiredMachines: 463241,
  groupId: "<id>",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `capacityBlocker`                                                                  | [models.ComputeCapacityBlocker](../models/computecapacityblocker.md)               | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `currentMachines`                                                                  | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `desiredMachines`                                                                  | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `drainProgress`                                                                    | [models.ComputeDrainProgress](../models/computedrainprogress.md)                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `groupId`                                                                          | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `instanceType`                                                                     | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `maxMachines`                                                                      | *number*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `minMachines`                                                                      | *number*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `recommendation`                                                                   | [models.ComputeCapacityRecommendation](../models/computecapacityrecommendation.md) | :heavy_minus_sign:                                                                 | N/A                                                                                |