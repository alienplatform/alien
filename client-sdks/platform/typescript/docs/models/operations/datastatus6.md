# DataStatus6

## Example Usage

```typescript
import { DataStatus6 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus6 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "updating",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue6](../../models/operations/collectionissue6.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health6](../../models/operations/health6.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle6](../../models/operations/lifecycle6.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |