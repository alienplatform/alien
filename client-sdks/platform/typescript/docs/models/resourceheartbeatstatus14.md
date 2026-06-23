# ResourceHeartbeatStatus14

## Example Usage

```typescript
import { ResourceHeartbeatStatus14 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus14 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "scaling",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue14](../models/datacollectionissue14.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth14](../models/datahealth14.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle14](../models/statuslifecycle14.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |