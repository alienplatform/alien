# QueueHeartbeatDataLocal

## Example Usage

```typescript
import { QueueHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: QueueHeartbeatDataLocal = {
  name: "<value>",
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `name`                                                           | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `path`                                                           | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `serviceStatus`                                                  | *string*                                                         | :heavy_minus_sign:                                               | N/A                                                              |
| `status`                                                         | [models.QueueHeartbeatStatus](../models/queueheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"local"*                                                        | :heavy_check_mark:                                               | N/A                                                              |