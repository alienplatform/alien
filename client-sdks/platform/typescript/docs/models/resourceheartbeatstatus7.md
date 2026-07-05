# ResourceHeartbeatStatus7

## Example Usage

```typescript
import { ResourceHeartbeatStatus7 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus7 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "deleted",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `collectionIssues`                                                 | [models.DataCollectionIssue7](../models/datacollectionissue7.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `health`                                                           | [models.DataHealth7](../models/datahealth7.md)                     | :heavy_check_mark:                                                 | N/A                                                                |
| `lifecycle`                                                        | [models.StatusLifecycle7](../models/statuslifecycle7.md)           | :heavy_check_mark:                                                 | N/A                                                                |
| `message`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `partial`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `stale`                                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |