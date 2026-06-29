# CreateManagerResponsePoolsFixed3

## Example Usage

```typescript
import { CreateManagerResponsePoolsFixed3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponsePoolsFixed3 = {
  machines: 266101,
  mode: "fixed",
};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `machine`                                           | *string*                                            | :heavy_minus_sign:                                  | Provider machine type selected for this deployment. |
| `machines`                                          | *number*                                            | :heavy_check_mark:                                  | Number of machines to run.                          |
| `mode`                                              | *"fixed"*                                           | :heavy_check_mark:                                  | N/A                                                 |