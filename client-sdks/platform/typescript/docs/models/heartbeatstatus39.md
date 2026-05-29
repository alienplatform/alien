# HeartbeatStatus39

## Example Usage

```typescript
import { HeartbeatStatus39 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus39 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "failed",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue39](../models/collectionissue39.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health39](../models/health39.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle39](../models/statuslifecycle39.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |