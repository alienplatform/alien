# HeartbeatStatus23

## Example Usage

```typescript
import { HeartbeatStatus23 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus23 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "scaling",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue23](../models/collectionissue23.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health23](../models/health23.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle23](../models/statuslifecycle23.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |