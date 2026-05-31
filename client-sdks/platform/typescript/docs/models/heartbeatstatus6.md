# HeartbeatStatus6

## Example Usage

```typescript
import { HeartbeatStatus6 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus6 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "deleted",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue6](../models/collectionissue6.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health6](../models/health6.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle6](../models/statuslifecycle6.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |