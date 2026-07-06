# ResourceHeartbeatStatus64

## Example Usage

```typescript
import { ResourceHeartbeatStatus64 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus64 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "running",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue64](../models/datacollectionissue64.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth64](../models/datahealth64.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle64](../models/statuslifecycle64.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |