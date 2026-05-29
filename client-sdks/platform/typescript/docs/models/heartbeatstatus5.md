# HeartbeatStatus5

## Example Usage

```typescript
import { HeartbeatStatus5 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus5 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "updating",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue5](../models/collectionissue5.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health5](../models/health5.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle5](../models/statuslifecycle5.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |