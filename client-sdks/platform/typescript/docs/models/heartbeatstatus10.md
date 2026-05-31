# HeartbeatStatus10

## Example Usage

```typescript
import { HeartbeatStatus10 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus10 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue10](../models/collectionissue10.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health10](../models/health10.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle10](../models/statuslifecycle10.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |