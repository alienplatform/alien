# HeartbeatStatus15

## Example Usage

```typescript
import { HeartbeatStatus15 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus15 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "failed",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue15](../models/collectionissue15.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health15](../models/health15.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle15](../models/statuslifecycle15.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |