# HeartbeatStatus42

## Example Usage

```typescript
import { HeartbeatStatus42 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus42 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "unknown",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue42](../models/collectionissue42.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health42](../models/health42.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle42](../models/statuslifecycle42.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |