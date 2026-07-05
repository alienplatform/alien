# DataStatus64

## Example Usage

```typescript
import { DataStatus64 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus64 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "scaling",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue64](../../models/operations/collectionissue64.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health64](../../models/operations/health64.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle64](../../models/operations/lifecycle64.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |