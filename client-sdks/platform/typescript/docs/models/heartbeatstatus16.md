# HeartbeatStatus16

## Example Usage

```typescript
import { HeartbeatStatus16 } from "@alienplatform/platform-api/models";

let value: HeartbeatStatus16 = {
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
| `collectionIssues`                                           | [models.CollectionIssue16](../models/collectionissue16.md)[] | :heavy_check_mark:                                           | N/A                                                          |
| `health`                                                     | [models.Health16](../models/health16.md)                     | :heavy_check_mark:                                           | N/A                                                          |
| `lifecycle`                                                  | [models.StatusLifecycle16](../models/statuslifecycle16.md)   | :heavy_check_mark:                                           | N/A                                                          |
| `message`                                                    | *string*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `partial`                                                    | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |
| `stale`                                                      | *boolean*                                                    | :heavy_check_mark:                                           | N/A                                                          |