# PutValuesRequest

## Example Usage

```typescript
import { PutValuesRequest } from "@alienplatform/manager-api/models";

let value: PutValuesRequest = {
  values: [
    {
      kind: "plain",
      slot: "<value>",
      value: "<value>",
    },
  ],
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `values`                                   | [models.PutValue](../models/putvalue.md)[] | :heavy_check_mark:                         | N/A                                        |
