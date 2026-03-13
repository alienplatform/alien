# CapacityGroup

## Example Usage

```typescript
import { CapacityGroup } from "@alienplatform/platform-api/models/operations";

let value: CapacityGroup = {
  groupId: "<id>",
  machines: 77442,
  unhealthyMachines: 293357,
  utilizationPercent: 315.97,
  recommendation: {
    groupId: "<id>",
    currentMachines: 706673,
    desiredMachines: 492282,
    reason: "<value>",
  },
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `groupId`                                                              | *string*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `machines`                                                             | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `unhealthyMachines`                                                    | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `utilizationPercent`                                                   | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `recommendation`                                                       | [operations.Recommendation](../../models/operations/recommendation.md) | :heavy_check_mark:                                                     | N/A                                                                    |