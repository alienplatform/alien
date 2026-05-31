# HeartbeatStatus54

## Example Usage

```typescript
import { HeartbeatStatus54 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus54 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "stopped",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue54](../models/collectionissue54.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health54](../models/health54.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle54](../models/statuslifecycle54.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |