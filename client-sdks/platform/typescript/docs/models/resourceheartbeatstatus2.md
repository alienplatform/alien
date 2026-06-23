# ResourceHeartbeatStatus2

## Example Usage

```typescript
import { ResourceHeartbeatStatus2 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus2 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "deleted",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `collectionIssues`                                                 | [models.DataCollectionIssue2](../models/datacollectionissue2.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `health`                                                           | [models.DataHealth2](../models/datahealth2.md)                     | :heavy_check_mark:                                                 | N/A                                                                |
| `lifecycle`                                                        | [models.StatusLifecycle2](../models/statuslifecycle2.md)           | :heavy_check_mark:                                                 | N/A                                                                |
| `message`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `partial`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `stale`                                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |