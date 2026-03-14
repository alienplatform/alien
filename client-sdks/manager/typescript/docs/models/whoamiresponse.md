# WhoamiResponse

## Example Usage

```typescript
import { WhoamiResponse } from "@alienplatform/manager-api/models";

let value: WhoamiResponse = {
  id: "<id>",
  kind: "<value>",
  scope: {
    type: "<value>",
  },
};
```

## Fields

| Field                                      | Type                                       | Required                                   | Description                                |
| ------------------------------------------ | ------------------------------------------ | ------------------------------------------ | ------------------------------------------ |
| `id`                                       | *string*                                   | :heavy_check_mark:                         | N/A                                        |
| `kind`                                     | *string*                                   | :heavy_check_mark:                         | N/A                                        |
| `scope`                                    | [models.ScopeInfo](../models/scopeinfo.md) | :heavy_check_mark:                         | N/A                                        |