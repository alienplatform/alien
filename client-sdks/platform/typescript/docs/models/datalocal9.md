# DataLocal9

## Example Usage

```typescript
import { DataLocal9 } from "@alienplatform/platform-api/models";

let value: DataLocal9 = {
  configured: false,
  identity: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `configured`                                               | *boolean*                                                  | :heavy_check_mark:                                         | N/A                                                        |
| `identity`                                                 | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.HeartbeatStatus39](../models/heartbeatstatus39.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"local"*                                                  | :heavy_check_mark:                                         | N/A                                                        |