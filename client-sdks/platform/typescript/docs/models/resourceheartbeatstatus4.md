# ResourceHeartbeatStatus4

## Example Usage

```typescript
import { ResourceHeartbeatStatus4 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus4 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "creating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `collectionIssues`                                                 | [models.DataCollectionIssue4](../models/datacollectionissue4.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `health`                                                           | [models.DataHealth4](../models/datahealth4.md)                     | :heavy_check_mark:                                                 | N/A                                                                |
| `lifecycle`                                                        | [models.StatusLifecycle4](../models/statuslifecycle4.md)           | :heavy_check_mark:                                                 | N/A                                                                |
| `message`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `partial`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `stale`                                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |