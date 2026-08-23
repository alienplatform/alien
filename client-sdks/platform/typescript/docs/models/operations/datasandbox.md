# DataSandbox

## Example Usage

```typescript
import { DataSandbox } from "@alienplatform/platform-api/models/operations";

let value: DataSandbox = {
  data: {
    activeSessions: 473650,
    idlePods: 968711,
    namespace: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "deleted",
      partial: false,
      stale: true,
    },
    backend: "kubernetesPods",
  },
  resourceType: "sandbox",
};
```

## Fields

| Field                                                                                                                                                                                                                                      | Type                                                                                                                                                                                                                                       | Required                                                                                                                                                                                                                                   | Description                                                                                                                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `data`                                                                                                                                                                                                                                     | *operations.DataUnion18*                                                                                                                                                                                                                   | :heavy_check_mark:                                                                                                                                                                                                                         | Content-free telemetry about a sandbox's sessions.<br/><br/>Never anything from inside a session. A controller reaches only the cloud's management APIs,<br/>and the whole point of the resource is that the control plane cannot see what runs in it. |
| `resourceType`                                                                                                                                                                                                                             | *"sandbox"*                                                                                                                                                                                                                                | :heavy_check_mark:                                                                                                                                                                                                                         | N/A                                                                                                                                                                                                                                        |