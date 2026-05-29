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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: true,
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
| `status`                                                           | [operations.DataStatus56](../../models/operations/datastatus56.md) | :heavy_check_mark:                                                 | N/A                                                                |