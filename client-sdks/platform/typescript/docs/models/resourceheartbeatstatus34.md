# ResourceHeartbeatStatus34

## Example Usage

```typescript
import { ResourceHeartbeatStatus34 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus34 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "deleted",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue34](../models/datacollectionissue34.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth34](../models/datahealth34.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle34](../models/statuslifecycle34.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |