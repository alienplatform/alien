# HeartbeatStatus8

## Example Usage

```typescript
import { HeartbeatStatus8 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus8 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "deleted",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue8](../models/collectionissue8.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health8](../models/health8.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle8](../models/statuslifecycle8.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |