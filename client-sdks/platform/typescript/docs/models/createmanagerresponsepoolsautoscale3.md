# CreateManagerResponsePoolsAutoscale3

## Example Usage

```typescript
import { CreateManagerResponsePoolsAutoscale3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePoolsAutoscale3 = {
  max: 300353,
  min: 199789,
  mode: "autoscale",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `failureDomains`                                    | *models.CreateManagerResponseFailureDomainsUnion6*  | :heavy_minus_sign:                                  | N/A                                                 |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `max`                                               | *number*                                            | :heavy_check_mark:                                  | Maximum machine count.                              |
| `min`                                               | *number*                                            | :heavy_check_mark:                                  | Minimum machine count.                              |
| `mode`                                              | *"autoscale"*                                       | :heavy_check_mark:                                  | N/A                                                 |
