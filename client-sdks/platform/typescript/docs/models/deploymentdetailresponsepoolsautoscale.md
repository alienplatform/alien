# DeploymentDetailResponsePoolsAutoscale

## Example Usage

```typescript
import { DeploymentDetailResponsePoolsAutoscale } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponsePoolsAutoscale = {
  max: 915259,
  min: 566451,
  mode: "autoscale",
};
```

## Fields

| Field                                                 | Type                                                  | Required                                              | Description                                           |
| ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------- |
| `failureDomains`                                      | *models.DeploymentDetailResponseFailureDomainsUnion2* | :heavy_minus_sign:                                    | N/A                                                   |
| `machine`                                             | *string*                                              | :heavy_minus_sign:                                    | Provider machine type selected for this deployment.   |
| `max`                                                 | *number*                                              | :heavy_check_mark:                                    | Maximum machine count.                                |
| `min`                                                 | *number*                                              | :heavy_check_mark:                                    | Minimum machine count.                                |
| `mode`                                                | *"autoscale"*                                         | :heavy_check_mark:                                    | N/A                                                   |
