# ResourceHeartbeatStatus58

## Example Usage

```typescript
import { ResourceHeartbeatStatus58 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus58 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "running",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `collectionIssues`                                                   | [models.DataCollectionIssue58](../models/datacollectionissue58.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |
| `health`                                                             | [models.DataHealth58](../models/datahealth58.md)                     | :heavy_check_mark:                                                   | N/A                                                                  |
| `lifecycle`                                                          | [models.StatusLifecycle58](../models/statuslifecycle58.md)           | :heavy_check_mark:                                                   | N/A                                                                  |
| `message`                                                            | *string*                                                             | :heavy_minus_sign:                                                   | N/A                                                                  |
| `partial`                                                            | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |
| `stale`                                                              | *boolean*                                                            | :heavy_check_mark:                                                   | N/A                                                                  |