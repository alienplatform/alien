# HeartbeatStatus61

## Example Usage

```typescript
import { HeartbeatStatus61 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus61 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "deleted",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue61](../models/collectionissue61.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health61](../models/health61.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle61](../models/statuslifecycle61.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |