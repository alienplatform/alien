# ResourceHeartbeatStatus41

## Example Usage

```typescript
import { ResourceHeartbeatStatus41 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus41 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "failed",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue41](../models/datacollectionissue41.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth41](../models/datahealth41.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle41](../models/statuslifecycle41.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |