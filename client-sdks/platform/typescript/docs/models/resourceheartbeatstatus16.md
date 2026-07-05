# ResourceHeartbeatStatus16

## Example Usage

```typescript
import { ResourceHeartbeatStatus16 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus16 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "updating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue16](../models/datacollectionissue16.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth16](../models/datahealth16.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle16](../models/statuslifecycle16.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |