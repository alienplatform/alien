# DataLocal8

Local embedded Postgres backend.

## Example Usage

```typescript
import { DataLocal8 } from "@alienplatform/platform-api/models";

let value: DataLocal8 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: true,
    stale: false,
  },
  version: "<value>",
  backend: "local",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `name`                                                                     | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `port`                                                                     | *number*                                                                   | :heavy_minus_sign:                                                         | N/A                                                                        |
| `processRunning`                                                           | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus36](../models/resourceheartbeatstatus36.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `version`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |