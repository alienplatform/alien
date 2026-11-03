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

| Field                                                                                                                                      | Type                                                                                                                                       | Required                                                                                                                                   | Description                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `dataDiskGib`                                                                                                                              | *number*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Data disk size per machine, in whole GiB, within the pool's declared<br/>range. Omitted uses the release default; 0 runs the pool without one. |
| `failureDomains`                                                                                                                           | [models.FailureDomainSelection](../models/failuredomainselection.md)                                                                       | :heavy_minus_sign:                                                                                                                         | N/A                                                                                                                                        |
| `machine`                                                                                                                                  | *string*                                                                                                                                   | :heavy_minus_sign:                                                                                                                         | Provider machine type selected for this deployment.                                                                                        |
| `max`                                                                                                                                      | *number*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Maximum machine count.                                                                                                                     |
| `min`                                                                                                                                      | *number*                                                                                                                                   | :heavy_check_mark:                                                                                                                         | Minimum machine count.                                                                                                                     |
| `mode`                                                                                                                                     | *"autoscale"*                                                                                                                              | :heavy_check_mark:                                                                                                                         | N/A                                                                                                                                        |
