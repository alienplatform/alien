# HeartbeatStatus28

## Example Usage

```typescript
import { HeartbeatStatus28 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus28 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "creating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue28](../models/collectionissue28.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health28](../models/health28.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle28](../models/statuslifecycle28.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |