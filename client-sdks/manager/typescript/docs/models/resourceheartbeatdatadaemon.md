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
        ready: true,
        replicaId: "<id>",
      },
    ],
    daemonName: "<value>",
    desiredMachines: 633734,
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    healthyInstances: 102281,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    unavailableInstances: 222122,
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