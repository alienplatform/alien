# HeartbeatStatus35

## Example Usage

```typescript
import { HeartbeatStatus35 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus35 = {
  collectionIssues: [],
  health: "unknown",
  lifecycle: "unknown",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `collectionIssues`                                           | [models.CollectionIssue35](../models/collectionissue35.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health35](../models/health35.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle35](../models/statuslifecycle35.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |