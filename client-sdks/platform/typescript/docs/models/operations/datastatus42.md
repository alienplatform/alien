# DataStatus42

## Example Usage

```typescript
import { DataStatus42 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus42 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "unknown",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue42](../../models/operations/collectionissue42.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health42](../../models/operations/health42.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle42](../../models/operations/lifecycle42.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |