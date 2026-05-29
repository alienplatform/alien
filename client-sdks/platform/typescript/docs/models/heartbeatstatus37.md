# HeartbeatStatus37

## Example Usage

```typescript
import { HeartbeatStatus37 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus37 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "failed",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue37](../models/collectionissue37.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health37](../models/health37.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle37](../models/statuslifecycle37.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |