# CapacityBlocker2

## Example Usage

```typescript
import { CapacityBlocker2 } from "@alienplatform/platform-api/models";

let value: CapacityBlocker2 = {
  category: "capacity",
  message: "<value>",
  observedAt: new Date("2025-07-31T07:57:02.198Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `category`                                                                                    | [models.Category2](../models/category2.md)                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerCode`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `providerReference`                                                                           | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |