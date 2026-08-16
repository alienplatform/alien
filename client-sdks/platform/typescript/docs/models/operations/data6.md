# Data6

## Example Usage

```typescript
import { Data6 } from "@alienplatform/platform-api/models/operations";

let value: Data6 = {
  enabled: false,
  keyArn: "<value>",
  keySpec: "<value>",
  keyState: "<value>",
  keyUsage: "<value>",
  status: {
    health: "healthy",
    lifecycle: "failed",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `enabled`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `keyArn`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `keySpec`                                                          | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `keyState`                                                         | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `keyUsage`                                                         | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus70](../../models/operations/datastatus70.md) | :heavy_check_mark:                                                 | N/A                                                                |