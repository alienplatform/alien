# Recommendation3

## Example Usage

```typescript
import { Recommendation3 } from "@alienplatform/platform-api/models";

let value: Recommendation3 = {
  desiredMachines: 119817,
};
```

## Fields

| Field                      | Type                       | Required                   | Description                |
| -------------------------- | -------------------------- | -------------------------- | -------------------------- |
| `desiredMachines`          | *number*                   | :heavy_check_mark:         | N/A                        |
| `reason`                   | *string*                   | :heavy_minus_sign:         | N/A                        |
| `unschedulableReplicas`    | *number*                   | :heavy_minus_sign:         | N/A                        |
| `utilization`              | *models.UtilizationUnion3* | :heavy_minus_sign:         | N/A                        |