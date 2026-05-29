# HeartbeatStatus59

## Example Usage

```typescript
import { HeartbeatStatus59 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus59 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "api-unavailable",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "updating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue59](../models/collectionissue59.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health59](../models/health59.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle59](../models/statuslifecycle59.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |