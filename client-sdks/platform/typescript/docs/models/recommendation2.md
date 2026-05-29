# Recommendation2

## Example Usage

```typescript
import { Recommendation2 } from "@alienplatform/platform-api/models";

let value: Recommendation2 = {
  desiredMachines: 262985,
};
```

## Fields

| Field                      | Type                       | Required                   | Description                |
| -------------------------- | -------------------------- | -------------------------- | -------------------------- |
| `desiredMachines`          | *number*                   | :heavy_check_mark:         | N/A                        |
| `reason`                   | *string*                   | :heavy_minus_sign:         | N/A                        |
| `unschedulableReplicas`    | *number*                   | :heavy_minus_sign:         | N/A                        |
| `utilization`              | *models.UtilizationUnion2* | :heavy_minus_sign:         | N/A                        |