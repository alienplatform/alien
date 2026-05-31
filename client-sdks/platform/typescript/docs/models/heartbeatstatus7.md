# HeartbeatStatus7

## Example Usage

```typescript
import { HeartbeatStatus7 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus7 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "stopped",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue7](../models/collectionissue7.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health7](../models/health7.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle7](../models/statuslifecycle7.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |