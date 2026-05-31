# DataStatus32

## Example Usage

```typescript
import { DataStatus32 } from "@alienplatform/platform-api/models/operations";

let value: DataStatus32 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "running",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `collectionIssues`                                                             | [operations.CollectionIssue32](../../models/operations/collectionissue32.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `health`                                                                       | [operations.Health32](../../models/operations/health32.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `lifecycle`                                                                    | [operations.Lifecycle32](../../models/operations/lifecycle32.md)               | :heavy_check_mark:                                                             | N/A                                                                            |
| `message`                                                                      | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `partial`                                                                      | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `stale`                                                                        | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |