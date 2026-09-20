# Data2

## Example Usage

```typescript
import { Data2 } from "@alienplatform/platform-api/models/operations";

let value: Data2 = {
  managedTags: {},
  name: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `location`                                                         | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `managedTags`                                                      | Record<string, *string*>                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `provisioningState`                                                | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `resourceId`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus62](../../models/operations/datastatus62.md) | :heavy_check_mark:                                                 | N/A                                                                |