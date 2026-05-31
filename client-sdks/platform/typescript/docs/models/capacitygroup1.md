# CapacityGroup1

## Example Usage

```typescript
import { CapacityGroup1 } from "@alienplatform/platform-api/models";

let value: CapacityGroup1 = {
  currentMachines: 202194,
  desiredMachines: 760792,
  groupId: "<id>",
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `capacityBlocker`              | *models.CapacityBlockerUnion1* | :heavy_minus_sign:             | N/A                            |
| `currentMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `desiredMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `groupId`                      | *string*                       | :heavy_check_mark:             | N/A                            |
| `instanceType`                 | *string*                       | :heavy_minus_sign:             | N/A                            |
| `maxMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `minMachines`                  | *number*                       | :heavy_minus_sign:             | N/A                            |
| `recommendation`               | *models.RecommendationUnion1*  | :heavy_minus_sign:             | N/A                            |