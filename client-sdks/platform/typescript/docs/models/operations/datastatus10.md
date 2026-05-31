# DataStatus10

## Example Usage

```typescript
import { DataStatus10 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus10 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "scaling",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue10](../../models/operations/collectionissue10.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health10](../../models/operations/health10.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle10](../../models/operations/lifecycle10.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |