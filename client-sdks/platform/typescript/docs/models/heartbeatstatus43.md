# HeartbeatStatus43

## Example Usage

```typescript
import { HeartbeatStatus43 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus43 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue43](../models/collectionissue43.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health43](../models/health43.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle43](../models/statuslifecycle43.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |