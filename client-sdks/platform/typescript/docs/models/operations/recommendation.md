# Recommendation

## Example Usage

```typescript
import { Recommendation } from "@aliendotdev/platform-api/models/operations";

let value: Recommendation = {
  groupId: "<id>",
  currentMachines: 534206,
  desiredMachines: 397017,
  reason: "<value>",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `groupId`               | *string*                | :heavy_check_mark:      | N/A                     |
| `currentMachines`       | *number*                | :heavy_check_mark:      | N/A                     |
| `desiredMachines`       | *number*                | :heavy_check_mark:      | N/A                     |
| `reason`                | *string*                | :heavy_check_mark:      | N/A                     |
| `utilizationPercent`    | *number*                | :heavy_minus_sign:      | N/A                     |
| `unschedulableReplicas` | *number*                | :heavy_minus_sign:      | N/A                     |