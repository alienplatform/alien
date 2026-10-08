# CreateManagerResponsePoolsAutoscale1

## Example Usage

```typescript
import { CreateManagerResponsePoolsAutoscale1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePoolsAutoscale1 = {
  max: 25546,
  min: 145692,
  mode: "autoscale",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `failureDomains`                                    | *models.CreateManagerResponseFailureDomainsUnion2*  | :heavy_minus_sign:                                  | N/A                                                 |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `max`                                               | *number*                                            | :heavy_check_mark:                                  | Maximum machine count.                              |
| `min`                                               | *number*                                            | :heavy_check_mark:                                  | Minimum machine count.                              |
| `mode`                                              | *"autoscale"*                                       | :heavy_check_mark:                                  | N/A                                                 |
