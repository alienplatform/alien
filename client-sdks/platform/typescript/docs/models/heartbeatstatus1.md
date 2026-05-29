# HeartbeatStatus1

## Example Usage

```typescript
import { HeartbeatStatus1 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus1 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `collectionIssues`                                         | [models.CollectionIssue1](../models/collectionissue1.md)[] | :heavy_check_mark:                                         | N/A                                                        |
| `health`                                                   | [models.Health1](../models/health1.md)                     | :heavy_check_mark:                                         | N/A                                                        |
| `lifecycle`                                                | [models.StatusLifecycle1](../models/statuslifecycle1.md)   | :heavy_check_mark:                                         | N/A                                                        |
| `message`                                                  | *string*                                                   | :heavy_minus_sign:                                         | N/A                                                        |
| `partial`                                                  | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `stale`                                                    | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |