# Heartbeat

## Example Usage

```typescript
import { Heartbeat } from "@alienplatform/platform-api/models/operations";

let value: Heartbeat = {
  backend: "gcp",
  controllerPlatform: "kubernetes",
  data: {
    data: {
      assignedMachines: 349166,
      capacityGroup: "<value>",
      commandSupported: true,
      daemonInstances: [
        {
          name: "<value>",
          ready: true,
          replicaId: "<id>",
        },
      ],
      desiredMachines: 24926,
      events: [],
      healthyInstances: 426500,
      horizonClusterId: "<id>",
      horizonStatus: "<value>",
      latestUpdateTimestamp: "<value>",
      status: {
        collectionIssues: [
          {
            message: "<value>",
            reason: "not-installed",
            severity: "warning",
            source: "<value>",
          },
        ],
        health: "unknown",
        lifecycle: "running",
        partial: false,
        stale: false,
      },
      unavailableInstances: 461497,
      backend: "azure",
    },
    resourceType: "daemon",
  },
  observedAt: new Date("2024-11-22T15:55:34.023Z"),
  raw: [
    {
      body: "<value>",
      collectedAt: new Date("2025-01-29T23:35:36.058Z"),
      format: "json",
      source: "<value>",
      truncated: true,
    },
  ],
  resourceId: "<id>",
  resourceType: "<value>",
};
```

## Fields

| Field                                                                                                                                                      | Type                                                                                                                                                       | Required                                                                                                                                                   | Description                                                                                                                                                |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `backend`                                                                                                                                                  | [operations.BackendEnum](../../models/operations/backendenum.md)                                                                                           | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `controllerPlatform`                                                                                                                                       | [operations.ControllerPlatform](../../models/operations/controllerplatform.md)                                                                             | :heavy_check_mark:                                                                                                                                         | Represents the target cloud platform.                                                                                                                      |
| `data`                                                                                                                                                     | *operations.DataUnion15*                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `deploymentId`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |
| `observedAt`                                                                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                              | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `raw`                                                                                                                                                      | [operations.Raw](../../models/operations/raw.md)[]                                                                                                         | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `resourceId`                                                                                                                                               | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Alien resource id, such as the `alien.Container` or `alien.Storage`<br/>resource id from the stack.                                                        |
| `resourceType`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |