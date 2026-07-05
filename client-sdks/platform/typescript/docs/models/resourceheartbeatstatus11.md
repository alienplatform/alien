# ResourceHeartbeatStatus11

## Example Usage

```typescript
import { ResourceHeartbeatStatus11 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus11 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "unknown",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue11](../models/datacollectionissue11.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth11](../models/datahealth11.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle11](../models/statuslifecycle11.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |