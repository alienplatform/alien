# ManagerRetryResponsePoolsFixed1

## Example Usage

```typescript
import { ManagerRetryResponsePoolsFixed1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponsePoolsFixed1 = {
  machines: 793634,
  mode: "fixed",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `failureDomains`                                    | *models.ManagerRetryResponseFailureDomainsUnion1*   | :heavy_minus_sign:                                  | N/A                                                 |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `machines`                                          | *number*                                            | :heavy_check_mark:                                  | Number of machines to run.                          |
| `mode`                                              | *"fixed"*                                           | :heavy_check_mark:                                  | N/A                                                 |
