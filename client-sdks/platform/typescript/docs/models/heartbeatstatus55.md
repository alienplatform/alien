# HeartbeatStatus55

## Example Usage

```typescript
import { HeartbeatStatus55 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus55 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue55](../models/collectionissue55.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health55](../models/health55.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle55](../models/statuslifecycle55.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |