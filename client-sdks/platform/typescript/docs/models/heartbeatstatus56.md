# HeartbeatStatus56

## Example Usage

```typescript
import { HeartbeatStatus56 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus56 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "failed",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue56](../models/collectionissue56.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health56](../models/health56.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle56](../models/statuslifecycle56.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |