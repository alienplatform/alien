# AiHeartbeatDataGcpVertex

## Example Usage

```typescript
import { AiHeartbeatDataGcpVertex } from "@alienplatform/manager-api/models";

let value: AiHeartbeatDataGcpVertex = {
  location: "<value>",
  project: "<value>",
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
  backend: "gcpVertex",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `location`                                                 | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `project`                                                  | *string*                                                   | :heavy_check_mark:                                         | N/A                                                        |
| `status`                                                   | [models.AiHeartbeatStatus](../models/aiheartbeatstatus.md) | :heavy_check_mark:                                         | N/A                                                        |
| `backend`                                                  | *"gcpVertex"*                                              | :heavy_check_mark:                                         | N/A                                                        |
