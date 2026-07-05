# ResourceHeartbeatStatus54

## Example Usage

```typescript
import { ResourceHeartbeatStatus54 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus54 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "updating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue54](../models/datacollectionissue54.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth54](../models/datahealth54.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle54](../models/statuslifecycle54.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |