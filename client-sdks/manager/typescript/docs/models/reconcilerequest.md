# ReconcileRequest

## Example Usage

```typescript
import { ReconcileRequest } from "@alienplatform/manager-api/models";

let value: ReconcileRequest = {
  deploymentId: "<id>",
  heartbeats: [
    {
      backend: "azure",
      controllerPlatform: "local",
      data: {
        data: {
          assignedMachines: 656017,
          capacityGroup: "<value>",
          commandSupported: false,
          daemonName: "<value>",
          desiredMachines: 546242,
          events: [],
          healthyInstances: 8454,
          horizonClusterId: "<id>",
          horizonStatus: "<value>",
          instances: [
            {
              name: "<value>",
              ready: true,
              replicaId: "<id>",
            },
          ],
          latestUpdateTimestamp: "<value>",
          status: {
            collectionIssues: [],
            health: "unknown",
            lifecycle: "running",
            partial: false,
            stale: true,
          },
          unavailableInstances: 63902,
          backend: "aws",
        },
        resourceType: "daemon",
      },
      observedAt: new Date("2026-09-19T23:42:23.532Z"),
      raw: [
        {
          body: "<value>",
          collectedAt: new Date("2024-03-28T05:55:21.668Z"),
          format: "json",
          source: "<value>",
          truncated: true,
        },
      ],
      resourceId: "<id>",
      resourceType: "worker",
    },
  ],
  session: "<value>",
  state: "South Carolina",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `deploymentId`                                               | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `error`                                                      | *any*                                                        | :heavy_minus_sign:                                           | N/A                                                          |
| `heartbeats`                                                 | [models.ResourceHeartbeat](../models/resourceheartbeat.md)[] | :heavy_minus_sign:                                           | N/A                                                          |
| `session`                                                    | *string*                                                     | :heavy_check_mark:                                           | N/A                                                          |
| `state`                                                      | *any*                                                        | :heavy_check_mark:                                           | N/A                                                          |
| `suggestedDelayMs`                                           | *number*                                                     | :heavy_minus_sign:                                           | N/A                                                          |
| `updateHeartbeat`                                            | *boolean*                                                    | :heavy_minus_sign:                                           | N/A                                                          |