# HeartbeatStatus62

## Example Usage

```typescript
import { HeartbeatStatus62 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus62 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "scaling",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue62](../models/collectionissue62.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health62](../models/health62.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle62](../models/statuslifecycle62.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |