# PlanDeploymentComputePoolsAutoscale

## Example Usage

```typescript
import { PlanDeploymentComputePoolsAutoscale } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputePoolsAutoscale = {
  max: 546442,
  min: 23862,
  mode: "autoscale",
};
```

## Fields

| Field                                                  | Type                                                   | Required                                               | Description                                            |
| ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------ |
| `failureDomains`                                       | *operations.PlanDeploymentComputeFailureDomainsUnion2* | :heavy_minus_sign:                                     | N/A                                                    |
| `machine`                                              | *string*                                               | :heavy_minus_sign:                                     | Provider machine type selected for this deployment.    |
| `max`                                                  | *number*                                               | :heavy_check_mark:                                     | Maximum machine count.                                 |
| `min`                                                  | *number*                                               | :heavy_check_mark:                                     | Minimum machine count.                                 |
| `mode`                                                 | *"autoscale"*                                          | :heavy_check_mark:                                     | N/A                                                    |