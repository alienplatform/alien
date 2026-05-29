# HeartbeatStatus26

## Example Usage

```typescript
import { HeartbeatStatus26 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus26 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "running",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue26](../models/collectionissue26.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health26](../models/health26.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle26](../models/statuslifecycle26.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |