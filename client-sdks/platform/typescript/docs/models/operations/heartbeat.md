# Heartbeat

## Example Usage

```typescript
import { Heartbeat } from "@alienplatform/platform-api/models/operations";

let value: Heartbeat = {
  backend: "gcp",
  controllerPlatform: "machines",
  data: {
    data: {
      capacityGroups: [],
      name: "<value>",
      nodes: {},
      providerFleets: [],
      status: {
        collectionIssues: [
          {
            message: "<value>",
            reason: "timed-out",
            severity: "error",
            source: "<value>",
          },
        ],
        health: "degraded",
        lifecycle: "running",
        partial: true,
        stale: false,
      },
      backend: "azure",
    },
    resourceType: "compute-cluster",
  },
  observedAt: new Date("2026-01-21T00:49:16.597Z"),
  raw: [],
  resourceId: "<id>",
  resourceType: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend`                                                                                                                                                  | [operations.BackendEnum](../../models/operations/backendenum.md)                                                                                           | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `controllerPlatform`                                                                                                                                       | [operations.ControllerPlatform](../../models/operations/controllerplatform.md)                                                                             | :heavy_check_mark:                                                                                                                                         | Represents the target cloud platform.                                                                                                                      |
| `data`                                                                                                                                                     | *operations.DataUnion20*                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `deploymentId`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |
| `observedAt`                                                                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                              | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `raw`                                                                                                                                                      | [operations.Raw](../../models/operations/raw.md)[]                                                                                                         | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `resourceId`                                                                                                                                               | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Alien resource id, such as the `alien.Container` or `alien.Storage`<br/>resource id from the stack.                                                        |
| `resourceType`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |