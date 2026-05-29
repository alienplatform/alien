# HeartbeatStatus4

## Example Usage

```typescript
import { HeartbeatStatus4 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus4 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "updating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue4](../models/collectionissue4.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health4](../models/health4.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle4](../models/statuslifecycle4.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |