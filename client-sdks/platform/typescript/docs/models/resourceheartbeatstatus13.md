# ResourceHeartbeatStatus13

## Example Usage

```typescript
import { ResourceHeartbeatStatus13 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus13 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue13](../models/datacollectionissue13.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth13](../models/datahealth13.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle13](../models/statuslifecycle13.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |