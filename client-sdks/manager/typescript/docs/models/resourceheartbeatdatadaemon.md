# ResourceHeartbeatDataDaemon

## Example Usage

```typescript
import { ResourceHeartbeatDataDaemon } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataDaemon = {
  data: {
    assignedMachines: 351239,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonInstances: [
      {
        name: "<value>",
        ready: false,
        replicaId: "<id>",
      },
    ],
    desiredMachines: 920664,
    events: [],
    healthyInstances: 222122,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "failed",
      partial: false,
      stale: false,
    },
    unavailableInstances: 631428,
    backend: "azure",
  },
  resourceType: "daemon",
};
```

## Fields

| Field                        | Type                         | Required                     | Description                  |
| ---------------------------- | ---------------------------- | ---------------------------- | ---------------------------- |
| `data`                       | *models.DaemonHeartbeatData* | :heavy_check_mark:           | N/A                          |
| `resourceType`               | *"daemon"*                   | :heavy_check_mark:           | N/A                          |