# DataStatus11

## Example Usage

```typescript
import { DataStatus11 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus11 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "failed",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue11](../../models/operations/collectionissue11.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health11](../../models/operations/health11.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle11](../../models/operations/lifecycle11.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |