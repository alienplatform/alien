# HeartbeatStatus27

## Example Usage

```typescript
import { HeartbeatStatus27 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus27 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "stopping",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue27](../models/collectionissue27.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health27](../models/health27.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle27](../models/statuslifecycle27.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |