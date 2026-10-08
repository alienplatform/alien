# ImportSourcePoolsFixed

## Example Usage

```typescript
import { ImportSourcePoolsFixed } from "@alienplatform/platform-api/models";

let value: ImportSourcePoolsFixed = {
  machines: 231983,
  mode: "fixed",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `failureDomains`                                    | *models.ImportSourceFailureDomainsUnion1*           | :heavy_minus_sign:                                  | N/A                                                 |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `machines`                                          | *number*                                            | :heavy_check_mark:                                  | Number of machines to run.                          |
| `mode`                                              | *"fixed"*                                           | :heavy_check_mark:                                  | N/A                                                 |
