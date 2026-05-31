# DataStatus1

## Example Usage

```typescript
import { DataStatus1 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus1 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "deleted",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue1](../../models/operations/collectionissue1.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health1](../../models/operations/health1.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle1](../../models/operations/lifecycle1.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |