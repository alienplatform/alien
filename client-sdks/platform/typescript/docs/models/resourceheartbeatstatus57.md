# ResourceHeartbeatStatus57

## Example Usage

```typescript
import { ResourceHeartbeatStatus57 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus57 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "stopped",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue57](../models/datacollectionissue57.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth57](../models/datahealth57.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle57](../models/statuslifecycle57.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |