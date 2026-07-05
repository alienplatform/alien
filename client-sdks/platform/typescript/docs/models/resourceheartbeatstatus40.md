# ResourceHeartbeatStatus40

## Example Usage

```typescript
import { ResourceHeartbeatStatus40 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus40 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "updating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue40](../models/datacollectionissue40.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth40](../models/datahealth40.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle40](../models/statuslifecycle40.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |