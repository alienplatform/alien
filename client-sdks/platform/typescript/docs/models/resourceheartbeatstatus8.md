# ResourceHeartbeatStatus8

## Example Usage

```typescript
import { ResourceHeartbeatStatus8 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus8 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "stopping",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `collectionIssues`                                                 | [models.DataCollectionIssue8](../models/datacollectionissue8.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `health`                                                           | [models.DataHealth8](../models/datahealth8.md)                     | :heavy_check_mark:                                                 | N/A                                                                |
| `lifecycle`                                                        | [models.StatusLifecycle8](../models/statuslifecycle8.md)           | :heavy_check_mark:                                                 | N/A                                                                |
| `message`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `partial`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `stale`                                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |