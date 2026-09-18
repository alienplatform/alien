# CapacityGroup4

## Example Usage

```typescript
import { CapacityGroup4 } from "@alienplatform/platform-api/models/operations";

let value: CapacityGroup4 = {
  currentMachines: 139935,
  desiredMachines: 809024,
  groupId: "<id>",
};
```

## Fields

| Field                              | Type                               | Required                           | Description                        |
| ---------------------------------- | ---------------------------------- | ---------------------------------- | ---------------------------------- |
| `capacityBlocker`                  | *operations.CapacityBlockerUnion4* | :heavy_minus_sign:                 | N/A                                |
| `currentMachines`                  | *number*                           | :heavy_check_mark:                 | N/A                                |
| `desiredMachines`                  | *number*                           | :heavy_check_mark:                 | N/A                                |
| `drainProgress`                    | *operations.DrainProgressUnion4*   | :heavy_minus_sign:                 | N/A                                |
| `groupId`                          | *string*                           | :heavy_check_mark:                 | N/A                                |
| `instanceType`                     | *string*                           | :heavy_minus_sign:                 | N/A                                |
| `maxMachines`                      | *number*                           | :heavy_minus_sign:                 | N/A                                |
| `minMachines`                      | *number*                           | :heavy_minus_sign:                 | N/A                                |
| `recommendation`                   | *operations.RecommendationUnion4*  | :heavy_minus_sign:                 | N/A                                |