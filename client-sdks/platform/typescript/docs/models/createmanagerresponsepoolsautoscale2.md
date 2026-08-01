# CreateManagerResponsePoolsAutoscale2

## Example Usage

```typescript
import { CreateManagerResponsePoolsAutoscale2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePoolsAutoscale2 = {
  max: 819701,
  min: 837334,
  mode: "autoscale",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `max`                                               | *number*                                            | :heavy_check_mark:                                  | Maximum machine count.                              |
| `min`                                               | *number*                                            | :heavy_check_mark:                                  | Minimum machine count.                              |
| `mode`                                              | *"autoscale"*                                       | :heavy_check_mark:                                  | N/A                                                 |