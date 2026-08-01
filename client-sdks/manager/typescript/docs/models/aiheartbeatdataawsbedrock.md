# AiHeartbeatDataAwsBedrock

## Example Usage

```typescript
import { AiHeartbeatDataAwsBedrock } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataAwsBedrock = {
  region: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: false,
    stale: true,
  },
  backend: "awsBedrock",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `region`                                                   | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"awsBedrock"*                                             | :heavy_check_mark:                                         | N/A                                                        |
