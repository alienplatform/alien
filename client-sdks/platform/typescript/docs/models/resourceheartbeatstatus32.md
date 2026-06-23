# ResourceHeartbeatStatus32

## Example Usage

```typescript
import { ResourceHeartbeatStatus32 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus32 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "scaling",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue32](../models/datacollectionissue32.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth32](../models/datahealth32.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle32](../models/statuslifecycle32.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |