# ResourceHeartbeatStatus27

## Example Usage

```typescript
import { ResourceHeartbeatStatus27 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus27 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "running",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue27](../models/datacollectionissue27.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth27](../models/datahealth27.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle27](../models/statuslifecycle27.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |