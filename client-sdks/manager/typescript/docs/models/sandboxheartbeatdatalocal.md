# SandboxHeartbeatDataLocal

Local: containers Docker still holds for this sandbox.

## Example Usage

```typescript
import { SandboxHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: SandboxHeartbeatDataLocal = {
  activeSessions: 159901,
  routeServing: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `activeSessions`                                                                                                     | *number*                                                                                                             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `routeServing`                                                                                                       | *boolean*                                                                                                            | :heavy_check_mark:                                                                                                   | Whether the loopback route is serving in this process. False after a manager restart until<br/>the next tick rebinds it. |
| `status`                                                                                                             | [models.SandboxHeartbeatStatus](../models/sandboxheartbeatstatus.md)                                                 | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `backend`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |