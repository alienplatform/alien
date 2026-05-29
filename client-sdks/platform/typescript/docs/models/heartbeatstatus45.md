# HeartbeatStatus45

## Example Usage

```typescript
import { HeartbeatStatus45 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus45 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "scaling",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue45](../models/collectionissue45.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health45](../models/health45.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle45](../models/statuslifecycle45.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |