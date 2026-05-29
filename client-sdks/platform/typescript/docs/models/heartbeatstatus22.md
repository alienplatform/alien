# HeartbeatStatus22

## Example Usage

```typescript
import { HeartbeatStatus22 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus22 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "creating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue22](../models/collectionissue22.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health22](../models/health22.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle22](../models/statuslifecycle22.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |