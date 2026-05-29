# Heartbeat

## Example Usage

```typescript
import { Heartbeat } from "@alienplatform/platform-api/models";

let value: Heartbeat = {
  backend: "gcp",
  controllerPlatform: "kubernetes",
  data: {
    data: {
      assignedMachines: 349166,
      capacityGroup: "<value>",
      commandSupported: true,
      daemonName: "<value>",
      desiredMachines: 685250,
      events: [],
      healthyInstances: 253024,
      horizonClusterId: "<id>",
      horizonStatus: "<value>",
      instances: [],
      latestUpdateTimestamp: "<value>",
      status: {
        collectionIssues: [
          {
            message: "<value>",
            reason: "api-unavailable",
            severity: "info",
            source: "<value>",
          },
        ],
        health: "degraded",
        lifecycle: "unknown",
        partial: true,
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
| `backend`                                                                                                                                                  | [models.BackendEnum](../models/backendenum.md)                                                                                                             | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `controllerPlatform`                                                                                                                                       | [models.HeartbeatControllerPlatform](../models/heartbeatcontrollerplatform.md)                                                                             | :heavy_check_mark:                                                                                                                                         | Represents the target cloud platform.                                                                                                                      |
| `data`                                                                                                                                                     | *models.SyncReconcileRequestDataUnion15*                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `deploymentId`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_minus_sign:                                                                                                                                         | N/A                                                                                                                                                        |
| `observedAt`                                                                                                                                               | [Date](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/Date)                                                              | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `raw`                                                                                                                                                      | [models.Raw](../models/raw.md)[]                                                                                                                           | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `resourceId`                                                                                                                                               | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | N/A                                                                                                                                                        |
| `resourceType`                                                                                                                                             | *string*                                                                                                                                                   | :heavy_check_mark:                                                                                                                                         | Resource type identifier that determines the specific kind of resource. This field is used for polymorphic deserialization and resource-specific behavior. |