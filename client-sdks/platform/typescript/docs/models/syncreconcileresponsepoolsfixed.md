# SyncReconcileResponsePoolsFixed

## Example Usage

```typescript
import { SyncReconcileResponsePoolsFixed } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponsePoolsFixed = {
  machines: 346778,
  mode: "fixed",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `machines`                                          | *number*                                            | :heavy_check_mark:                                  | Number of machines to run.                          |
| `mode`                                              | *"fixed"*                                           | :heavy_check_mark:                                  | N/A                                                 |