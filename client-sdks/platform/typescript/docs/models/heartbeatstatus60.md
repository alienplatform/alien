# HeartbeatStatus60

## Example Usage

```typescript
import { HeartbeatStatus60 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus60 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "stopped",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue60](../models/collectionissue60.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health60](../models/health60.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle60](../models/statuslifecycle60.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |