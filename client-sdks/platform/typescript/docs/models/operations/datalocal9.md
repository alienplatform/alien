# DataLocal9

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal9 = {
  configured: false,
  identity: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `configured`                                                       | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `identity`                                                         | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus39](../../models/operations/datastatus39.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"local"*                                                          | :heavy_check_mark:                                                 | N/A                                                                |