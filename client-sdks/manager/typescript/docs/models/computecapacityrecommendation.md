# ComputeCapacityRecommendation

## Example Usage

```typescript
import { ComputeCapacityRecommendation } from "@alienplatform/manager-api/models";

let value: ComputeCapacityRecommendation = {
  desiredMachines: 939427,
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `desiredMachines`                                | *number*                                         | :heavy_check_mark:                               | N/A                                              |
| `reason`                                         | *string*                                         | :heavy_minus_sign:                               | N/A                                              |
| `unschedulableReplicas`                          | *number*                                         | :heavy_minus_sign:                               | N/A                                              |
| `utilization`                                    | [models.MetricSample](../models/metricsample.md) | :heavy_minus_sign:                               | N/A                                              |