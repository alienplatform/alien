# DataLocal9

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal9 = {
  configured: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-12-06T21:51:19.391Z"),
      severity: "error",
    },
  ],
  identity: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `configured`                                                                                             | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent39](../../models/operations/getrawresourceheartbeatevent39.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `identity`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus39](../../models/operations/datastatus39.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"local"*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |