# ResourceHeartbeatStatus38

## Example Usage

```typescript
import { ResourceHeartbeatStatus38 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus38 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "failed",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue38](../models/datacollectionissue38.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth38](../models/datahealth38.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle38](../models/statuslifecycle38.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |