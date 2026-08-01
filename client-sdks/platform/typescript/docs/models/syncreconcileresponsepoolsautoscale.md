# SyncReconcileResponsePoolsAutoscale

## Example Usage

```typescript
import { SyncReconcileResponsePoolsAutoscale } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePoolsAutoscale = {
  max: 152917,
  min: 940970,
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