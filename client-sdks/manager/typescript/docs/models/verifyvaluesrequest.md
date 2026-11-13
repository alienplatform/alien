# VerifyValuesRequest

## Example Usage

```typescript
import { VerifyValuesRequest } from "@alienplatform/manager-api/models";

let value: VerifyValuesRequest = {
  values: [
    {
      slot: "<value>",
      value: "<value>",
    },
  ],
};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `values`                                         | [models.VerifyValue](../models/verifyvalue.md)[] | :heavy_check_mark:                               | N/A                                              |
