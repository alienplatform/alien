# ResourceHeartbeatStatus20

## Example Usage

```typescript
import { ResourceHeartbeatStatus20 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus20 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "stopped",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue20](../models/datacollectionissue20.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth20](../models/datahealth20.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle20](../models/statuslifecycle20.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |