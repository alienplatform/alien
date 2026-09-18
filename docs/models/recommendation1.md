# Recommendation1

## Example Usage

```typescript
import { Recommendation1 } from "@alienplatform/platform-api/models";

let value: Recommendation1 = {
  desiredMachines: 180796,
};
```

## Fields

| Field                      | Type                       | Required                   | Description                |
| -------------------------- | -------------------------- | -------------------------- | -------------------------- |
| `desiredMachines`          | *number*                   | :heavy_check_mark:         | N/A                        |
| `reason`                   | *string*                   | :heavy_minus_sign:         | N/A                        |
| `unschedulableReplicas`    | *number*                   | :heavy_minus_sign:         | N/A                        |
| `utilization`              | *models.UtilizationUnion1* | :heavy_minus_sign:         | N/A                        |