# HeartbeatStatus31

## Example Usage

```typescript
import { HeartbeatStatus31 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus31 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "creating",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue31](../models/collectionissue31.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health31](../models/health31.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle31](../models/statuslifecycle31.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |