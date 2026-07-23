# ImportSourcePoolsAutoscale

## Example Usage

```typescript
import { ImportSourcePoolsAutoscale } from "@alienplatform/platform-api/models";

let value: ImportSourcePoolsAutoscale = {
  max: 222655,
  min: 556865,
  mode: "autoscale",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `failureDomains`                                    | *models.ImportSourceFailureDomainsUnion2*           | :heavy_minus_sign:                                  | N/A                                                 |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `max`                                               | *number*                                            | :heavy_check_mark:                                  | Maximum machine count.                              |
| `min`                                               | *number*                                            | :heavy_check_mark:                                  | Minimum machine count.                              |
| `mode`                                              | *"autoscale"*                                       | :heavy_check_mark:                                  | N/A                                                 |
