# ResourceHeartbeatStatus6

## Example Usage

```typescript
import { ResourceHeartbeatStatus6 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus6 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "updating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `collectionIssues`                                                 | [models.DataCollectionIssue6](../models/datacollectionissue6.md)[] | :heavy_check_mark:                                                 | N/A                                                                |
| `health`                                                           | [models.DataHealth6](../models/datahealth6.md)                     | :heavy_check_mark:                                                 | N/A                                                                |
| `lifecycle`                                                        | [models.StatusLifecycle6](../models/statuslifecycle6.md)           | :heavy_check_mark:                                                 | N/A                                                                |
| `message`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `partial`                                                          | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `stale`                                                            | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |