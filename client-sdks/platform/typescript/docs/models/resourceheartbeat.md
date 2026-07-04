# ResourceHeartbeat

## Example Usage

```typescript
import { ResourceHeartbeat } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeat = {
  backend: "aws",
  controllerPlatform: "kubernetes",
  data: {
    data: {
      commandSupported: false,
      events: [],
      name: "<value>",
      namespace: "<value>",
      pods: [],
      replicas: {},
      status: {
        collectionIssues: [],
        health: "degraded",
        lifecycle: "deleted",
        partial: true,
        stale: true,
      },
      backend: "kubernetes",
    },
    resourceType: "daemon",
  },
  observedAt: new Date("2026-05-08T10:59:50.217Z"),
  raw: [],
  resourceId: "<id>",
  resourceType: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend`                                                                                                                                                  | [models.ResourceHeartbeatBackendEnum](../models/resourceheartbeatbackendenum.md)                                                                           | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `controllerPlatform`                                                                                                                                       | [models.ResourceHeartbeatControllerPlatform](../models/resourceheartbeatcontrollerplatform.md)                                                             | :heavy_check_mark:                                                                                                                                         | Represents the target cloud platform.                                                                                                                      |
| `data`                                                                                                                                                     | *models.SyncReconcileRequestDataUnion16*                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `deploymentId`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |
| `observedAt`                                                                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                              | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `raw`                                                                                                                                                      | [models.ResourceHeartbeatRaw](../models/resourceheartbeatraw.md)[]                                                                                         | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `resourceId`                                                                                                                                               | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Alien resource id, such as the `alien.Container` or `alien.Storage`<br/>resource id from the stack.                                                        |
| `resourceType`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |