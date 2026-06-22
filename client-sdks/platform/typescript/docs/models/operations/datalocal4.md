# DataLocal4

## Example Usage

```typescript
import { DataLocal4 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal4 = {
  commandSupported: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      severity: "warning",
      timestamp: new Date("2026-01-17T09:27:27.938Z"),
    },
  ],
  imagePathPresent: true,
  runtimeId: "<id>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `commandSupported`                                                 | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `daemonInstance`                                                   | *operations.DaemonInstanceUnion*                                   | :heavy_minus_sign:                                                 | N/A                                                                |
| `daemonName`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `events`                                                           | [operations.Event10](../../models/operations/event10.md)[]         | :heavy_check_mark:                                                 | N/A                                                                |
| `exitReason`                                                       | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `imagePathPresent`                                                 | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `pid`                                                              | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `restartCount`                                                     | *number*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `runtimeId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus17](../../models/operations/datastatus17.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"local"*                                                          | :heavy_check_mark:                                                 | N/A                                                                |