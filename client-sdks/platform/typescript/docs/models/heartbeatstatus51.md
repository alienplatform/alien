# HeartbeatStatus51

## Example Usage

```typescript
import { HeartbeatStatus51 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus51 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "scaling",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue51](../models/collectionissue51.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health51](../models/health51.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle51](../models/statuslifecycle51.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |