# HeartbeatStatus11

## Example Usage

```typescript
import { HeartbeatStatus11 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus11 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "failed",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue11](../models/collectionissue11.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health11](../models/health11.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle11](../models/statuslifecycle11.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |