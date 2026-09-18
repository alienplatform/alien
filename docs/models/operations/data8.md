# Data8

## Example Usage

```typescript
import { Data8 } from "@alienplatform/platform-api/models/operations";

let value: Data8 = {
  keyId: "<id>",
  keyOperations: [
    "<value 1>",
    "<value 2>",
  ],
  keyType: "<value>",
  status: {
    health: "unhealthy",
    lifecycle: "deleting",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `enabled`                                                          | *boolean*                                                          | :heavy_minus_sign:                                                 | N/A                                                                |
| `keyId`                                                            | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `keyOperations`                                                    | *string*[]                                                         | :heavy_check_mark:                                                 | N/A                                                                |
| `keyType`                                                          | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `recoveryLevel`                                                    | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus72](../../models/operations/datastatus72.md) | :heavy_check_mark:                                                 | N/A                                                                |