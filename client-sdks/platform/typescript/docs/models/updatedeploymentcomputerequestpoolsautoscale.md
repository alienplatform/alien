# UpdateDeploymentComputeRequestPoolsAutoscale

## Example Usage

```typescript
import { UpdateDeploymentComputeRequestPoolsAutoscale } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentComputeRequestPoolsAutoscale = {
  max: 712106,
  min: 904477,
  mode: "autoscale",
};
```

## Fields

| Field                                                       | Type                                                        | Required                                                    | Description                                                 |
| ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- | ----------------------------------------------------------- |
| `failureDomains`                                            | *models.UpdateDeploymentComputeRequestFailureDomainsUnion2* | :heavy_minus_sign:                                          | N/A                                                         |
| `machine`                                                   | *string*                                                    | :heavy_minus_sign:                                          | Provider machine type selected for this deployment.         |
| `max`                                                       | *number*                                                    | :heavy_check_mark:                                          | Maximum machine count.                                      |
| `min`                                                       | *number*                                                    | :heavy_check_mark:                                          | Minimum machine count.                                      |
| `mode`                                                      | *"autoscale"*                                               | :heavy_check_mark:                                          | N/A                                                         |