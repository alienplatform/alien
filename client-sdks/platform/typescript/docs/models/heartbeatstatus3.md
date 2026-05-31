# HeartbeatStatus3

## Example Usage

```typescript
import { HeartbeatStatus3 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus3 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "stopping",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue3](../models/collectionissue3.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health3](../models/health3.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle3](../models/statuslifecycle3.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |