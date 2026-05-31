# HeartbeatStatus47

## Example Usage

```typescript
import { HeartbeatStatus47 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus47 = {
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

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue47](../models/collectionissue47.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health47](../models/health47.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle47](../models/statuslifecycle47.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |