# HeartbeatStatus24

## Example Usage

```typescript
import { HeartbeatStatus24 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus24 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "updating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue24](../models/collectionissue24.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health24](../models/health24.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle24](../models/statuslifecycle24.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |