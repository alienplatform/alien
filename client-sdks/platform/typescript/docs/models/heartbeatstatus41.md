# HeartbeatStatus41

## Example Usage

```typescript
import { HeartbeatStatus41 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus41 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "deleted",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue41](../models/collectionissue41.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health41](../models/health41.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle41](../models/statuslifecycle41.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |