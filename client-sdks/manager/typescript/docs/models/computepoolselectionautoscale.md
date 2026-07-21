# ComputePoolSelectionAutoscale

Autoscaling machine pool.

## Example Usage

```typescript
import { ComputePoolSelectionAutoscale } from "@alienplatform/manager-api/models";

let value: ComputePoolSelectionAutoscale = {
  max: 782299,
  min: 991794,
  mode: "autoscale",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `failureDomains`                                                     | [models.FailureDomainSelection](../models/failuredomainselection.md) | :heavy_minus_sign:                                                   | N/A                                                                  |
| `machine`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | Provider machine type selected for this deployment.                  |
| `max`                                                                | *number*                                                             | :heavy_check_mark:                                                   | Maximum machine count.                                               |
| `min`                                                                | *number*                                                             | :heavy_check_mark:                                                   | Minimum machine count.                                               |
| `mode`                                                               | *"autoscale"*                                                        | :heavy_check_mark:                                                   | N/A                                                                  |