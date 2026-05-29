# HeartbeatStatus20

## Example Usage

```typescript
import { HeartbeatStatus20 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus20 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "stopped",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue20](../models/collectionissue20.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health20](../models/health20.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle20](../models/statuslifecycle20.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |