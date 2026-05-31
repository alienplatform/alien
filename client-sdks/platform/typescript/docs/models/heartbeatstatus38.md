# HeartbeatStatus38

## Example Usage

```typescript
import { HeartbeatStatus38 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus38 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "running",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue38](../models/collectionissue38.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health38](../models/health38.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle38](../models/statuslifecycle38.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |