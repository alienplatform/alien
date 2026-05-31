# DataStatus14

## Example Usage

```typescript
import { DataStatus14 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus14 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "failed",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue14](../../models/operations/collectionissue14.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health14](../../models/operations/health14.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle14](../../models/operations/lifecycle14.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |