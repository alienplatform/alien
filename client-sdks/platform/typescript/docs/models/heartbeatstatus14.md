# HeartbeatStatus14

## Example Usage

```typescript
import { HeartbeatStatus14 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus14 = {
  collectionIssues: [],
  health: "unhealthy",
  lifecycle: "updating",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue14](../models/collectionissue14.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health14](../models/health14.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle14](../models/statuslifecycle14.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |