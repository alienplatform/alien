# ResourceHeartbeat

## Example Usage

```typescript
import { ResourceHeartbeat } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeat = {
  backend: "aws",
  controllerPlatform: "kubernetes",
  data: {
    data: {
      bindMountCount: 947766,
      events: [],
      portCount: 126023,
      runtimeReachable: false,
      status: {
        collectionIssues: [],
        health: "unknown",
        lifecycle: "running",
        partial: false,
        stale: true,
      },
      backend: "local",
    },
    resourceType: "container",
  },
  observedAt: new Date("2024-04-21T00:33:03.766Z"),
  raw: [],
  resourceId: "<id>",
  resourceType: "worker",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                | Example                                                                                                                                                    |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend`                                                                                                                                                  | [models.HeartbeatBackend](../models/heartbeatbackend.md)                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `controllerPlatform`                                                                                                                                       | [models.PlatformEnum](../models/platformenum.md)                                                                                                           | :heavy_check_mark:                                                                                                                                         | Represents the target cloud platform.                                                                                                                      |                                                                                                                                                            |
| `data`                                                                                                                                                     | *models.ResourceHeartbeatData*                                                                                                                             | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `deploymentId`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `observedAt`                                                                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                              | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `raw`                                                                                                                                                      | [models.RawHeartbeatSnippet](../models/rawheartbeatsnippet.md)[]                                                                                           | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `resourceId`                                                                                                                                               | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |                                                                                                                                                            |
| `resourceType`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. | worker                                                                                                                                                     |