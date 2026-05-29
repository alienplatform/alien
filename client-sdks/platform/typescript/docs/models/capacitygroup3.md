# CapacityGroup3

## Example Usage

```typescript
import { CapacityGroup3 } from "@alienplatform/platform-api/models";

let value: CapacityGroup3 = {
  currentMachines: 704669,
  desiredMachines: 937919,
  groupId: "<id>",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `currentMachines`             | *number*                      | :heavy_check_mark:            | N/A                           |
| `desiredMachines`             | *number*                      | :heavy_check_mark:            | N/A                           |
| `groupId`                     | *string*                      | :heavy_check_mark:            | N/A                           |
| `instanceType`                | *string*                      | :heavy_minus_sign:            | N/A                           |
| `maxMachines`                 | *number*                      | :heavy_minus_sign:            | N/A                           |
| `minMachines`                 | *number*                      | :heavy_minus_sign:            | N/A                           |
| `recommendation`              | *models.RecommendationUnion3* | :heavy_minus_sign:            | N/A                           |