# ResourceHeartbeatDataSandbox

## Example Usage

```typescript
import { ResourceHeartbeatDataSandbox } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataSandbox = {
  data: {
    activeSessions: 953383,
    routeServing: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "sandbox",
};
```

## Fields

| Field                                                                                                                                                                                                                                      | Type                                                                                                                                                                                                                                       | Required                                                                                                                                                                                                                                   | Description                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `data`                                                                                                                                                                                                                                     | *models.SandboxHeartbeatData*                                                                                                                                                                                                              | :heavy_check_mark:                                                                                                                                                                                                                         | Content-free telemetry about a sandbox's sessions.<br/><br/>Never anything from inside a session. A controller reaches only the cloud's management APIs,<br/>and the whole point of the resource is that the control plane cannot see what runs in it. |
| `resourceType`                                                                                                                                                                                                                             | *"sandbox"*                                                                                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                                                                                         | N/A                                                                                                                                                                                                                                        |