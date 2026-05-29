# HeartbeatStatus2

## Example Usage

```typescript
import { HeartbeatStatus2 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus2 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "stopped",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue2](../models/collectionissue2.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health2](../models/health2.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle2](../models/statuslifecycle2.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |