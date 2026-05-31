# HeartbeatStatus50

## Example Usage

```typescript
import { HeartbeatStatus50 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus50 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "stopped",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue50](../models/collectionissue50.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health50](../models/health50.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle50](../models/statuslifecycle50.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |