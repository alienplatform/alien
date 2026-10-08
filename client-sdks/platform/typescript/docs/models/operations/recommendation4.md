# Recommendation4

## Example Usage

```typescript
import { Recommendation4 } from "@alienplatform/platform-api/models/operations";

let value: Recommendation4 = {
  desiredMachines: 621359,
};
```

## Fields

| Field                          | Type                           | Required                       | Description                    |
| ------------------------------ | ------------------------------ | ------------------------------ | ------------------------------ |
| `desiredMachines`              | *number*                       | :heavy_check_mark:             | N/A                            |
| `reason`                       | *string*                       | :heavy_minus_sign:             | N/A                            |
| `unschedulableReplicas`        | *number*                       | :heavy_minus_sign:             | N/A                            |
| `utilization`                  | *operations.UtilizationUnion4* | :heavy_minus_sign:             | N/A                            |