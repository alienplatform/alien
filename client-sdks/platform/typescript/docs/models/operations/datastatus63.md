# DataStatus63

## Example Usage

```typescript
import { DataStatus63 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus63 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue63](../../models/operations/collectionissue63.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health63](../../models/operations/health63.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle63](../../models/operations/lifecycle63.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |