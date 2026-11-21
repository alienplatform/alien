# DataLocal9

Local Valkey container.

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models";

let value: DataLocal9 = {
  name: "<value>",
  processRunning: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopped",
    partial: false,
    stale: true,
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
| `status`                                                                   | [models.ResourceHeartbeatStatus40](../models/resourceheartbeatstatus40.md) | :heavy_check_mark:                                                         | Same fields as every other resource heartbeat status.                      |
| `version`                                                                  | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"local"*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |