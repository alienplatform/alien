# CapacityGroup2

## Example Usage

```typescript
import { CapacityGroup2 } from "@alienplatform/platform-api/models/operations";

let value: CapacityGroup2 = {
  currentMachines: 63280,
  desiredMachines: 273320,
  groupId: "<id>",
};
```

## Fields

| Field                             | Type                              | Required                          | Description                       |
| --------------------------------- | --------------------------------- | --------------------------------- | --------------------------------- |
| `currentMachines`                 | *number*                          | :heavy_check_mark:                | N/A                               |
| `desiredMachines`                 | *number*                          | :heavy_check_mark:                | N/A                               |
| `groupId`                         | *string*                          | :heavy_check_mark:                | N/A                               |
| `instanceType`                    | *string*                          | :heavy_minus_sign:                | N/A                               |
| `maxMachines`                     | *number*                          | :heavy_minus_sign:                | N/A                               |
| `minMachines`                     | *number*                          | :heavy_minus_sign:                | N/A                               |
| `recommendation`                  | *operations.RecommendationUnion2* | :heavy_minus_sign:                | N/A                               |