# HeartbeatStatus18

## Example Usage

```typescript
import { HeartbeatStatus18 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus18 = {
  collectionIssues: [],
  health: "degraded",
  lifecycle: "creating",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue18](../models/collectionissue18.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health18](../models/health18.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle18](../models/statuslifecycle18.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |