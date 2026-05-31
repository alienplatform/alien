# HeartbeatStatus58

## Example Usage

```typescript
import { HeartbeatStatus58 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus58 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue58](../models/collectionissue58.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health58](../models/health58.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle58](../models/statuslifecycle58.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |