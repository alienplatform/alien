# HeartbeatStatus32

## Example Usage

```typescript
import { HeartbeatStatus32 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus32 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "running",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue32](../models/collectionissue32.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health32](../models/health32.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle32](../models/statuslifecycle32.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |