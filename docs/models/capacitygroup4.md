# CapacityGroup4

## Example Usage

```typescript
import { CapacityGroup4 } from "@alienplatform/platform-api/models";

let value: CapacityGroup4 = {
  currentMachines: 139935,
  desiredMachines: 809024,
  groupId: "<id>",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `capacityBlocker`              | *models.CapacityBlockerUnion4* | :heavy_minus_sign:             | N/A                            |
| `currentMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `desiredMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `drainProgress`                | *models.DrainProgressUnion4*   | :heavy_minus_sign:             | N/A                            |
| `groupId`                      | *string*                       | :heavy_check_mark:             | N/A                            |
| `instanceType`                 | *string*                       | :heavy_minus_sign:             | N/A                            |
| `maxMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `minMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `recommendation`               | *models.RecommendationUnion4*  | :heavy_minus_sign:             | N/A                            |