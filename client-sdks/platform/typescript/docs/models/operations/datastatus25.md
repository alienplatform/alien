# DataStatus25

## Example Usage

```typescript
import { DataStatus25 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus25 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "stopped",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue25](../../models/operations/collectionissue25.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health25](../../models/operations/health25.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle25](../../models/operations/lifecycle25.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |