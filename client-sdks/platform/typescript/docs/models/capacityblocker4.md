# CapacityBlocker4

## Example Usage

```typescript
import { CapacityBlocker4 } from "@alienplatform/platform-api/models";

let value: CapacityBlocker4 = {
  category: "quota",
  message: "<value>",
  observedAt: new Date("2026-09-19T17:43:59.200Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `category`                                                                                    | [models.Category4](../models/category4.md)                                                    | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerCode`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `providerReference`                                                                           | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |