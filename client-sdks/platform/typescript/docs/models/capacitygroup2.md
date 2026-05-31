# CapacityGroup2

## Example Usage

```typescript
import { CapacityGroup2 } from "@alienplatform/platform-api/models";

let value: CapacityGroup2 = {
  currentMachines: 63280,
  desiredMachines: 273320,
  groupId: "<id>",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `capacityBlocker`              | *models.CapacityBlockerUnion2* | :heavy_minus_sign:             | N/A                            |
| `currentMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `desiredMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `groupId`                      | *string*                       | :heavy_check_mark:             | N/A                            |
| `instanceType`                 | *string*                       | :heavy_minus_sign:             | N/A                            |
| `maxMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `minMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `recommendation`               | *models.RecommendationUnion2*  | :heavy_minus_sign:             | N/A                            |