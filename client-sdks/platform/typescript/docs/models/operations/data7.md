# Data7

## Example Usage

```typescript
import { Data7 } from "@alienplatform/platform-api/models/operations";

let value: Data7 = {
  cryptoKeyName: "<value>",
  purpose: "<value>",
  status: {
    health: "healthy",
    lifecycle: "deleted",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `algorithm`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `cryptoKeyName`                                                    | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `primaryState`                                                     | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `primaryVersion`                                                   | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `purpose`                                                          | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus71](../../models/operations/datastatus71.md) | :heavy_check_mark:                                                 | N/A                                                                |