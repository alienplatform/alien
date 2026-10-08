# CapacityBlocker3

## Example Usage

```typescript
import { CapacityBlocker3 } from "@alienplatform/platform-api/models/operations";

let value: CapacityBlocker3 = {
  category: "quota",
  message: "<value>",
  observedAt: new Date("2026-06-17T08:33:30.974Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `category`                                                                                    | [operations.Category3](../../models/operations/category3.md)                                  | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerCode`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `providerReference`                                                                           | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |