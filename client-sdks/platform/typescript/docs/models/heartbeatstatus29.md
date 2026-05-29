# HeartbeatStatus29

## Example Usage

```typescript
import { HeartbeatStatus29 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus29 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "updating",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue29](../models/collectionissue29.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health29](../models/health29.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle29](../models/statuslifecycle29.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |