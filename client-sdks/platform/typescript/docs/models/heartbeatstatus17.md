# HeartbeatStatus17

## Example Usage

```typescript
import { HeartbeatStatus17 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus17 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue17](../models/collectionissue17.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health17](../models/health17.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle17](../models/statuslifecycle17.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |