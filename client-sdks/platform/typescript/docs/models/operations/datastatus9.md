# DataStatus9

## Example Usage

```typescript
import { DataStatus9 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus9 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "deleting",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `collectionIssues`                                                           | [operations.CollectionIssue9](../../models/operations/collectionissue9.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `health`                                                                     | [operations.Health9](../../models/operations/health9.md)                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `lifecycle`                                                                  | [operations.Lifecycle9](../../models/operations/lifecycle9.md)               | :heavy_check_mark:                                                           | N/A                                                                          |
| `message`                                                                    | *string*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `partial`                                                                    | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `stale`                                                                      | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |