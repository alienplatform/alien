# HeartbeatStatus52

## Example Usage

```typescript
import { HeartbeatStatus52 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus52 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "deleted",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue52](../models/collectionissue52.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health52](../models/health52.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle52](../models/statuslifecycle52.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |