# ComputeCapacityBlocker

## Example Usage

```typescript
import { ComputeCapacityBlocker } from "@alienplatform/manager-api/models";

let value: ComputeCapacityBlocker = {
  category: "capacity",
  message: "<value>",
  observedAt: new Date("2025-05-06T05:39:21.388Z"),
};
```

## Fields

| Field                                                                                         | Type                                                                                          | Required                                                                                      | Description                                                                                   |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `category`                                                                                    | [models.ComputeCapacityBlockerCategory](../models/computecapacityblockercategory.md)          | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `message`                                                                                     | *string*                                                                                      | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `observedAt`                                                                                  | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date) | :heavy_check_mark:                                                                            | N/A                                                                                           |
| `providerCode`                                                                                | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |
| `providerReference`                                                                           | *string*                                                                                      | :heavy_minus_sign:                                                                            | N/A                                                                                           |