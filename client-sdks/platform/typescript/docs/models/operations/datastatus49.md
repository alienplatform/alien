# DataStatus49

## Example Usage

```typescript
import { DataStatus49 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus49 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "stopping",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue49](../../models/operations/collectionissue49.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health49](../../models/operations/health49.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle49](../../models/operations/lifecycle49.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |