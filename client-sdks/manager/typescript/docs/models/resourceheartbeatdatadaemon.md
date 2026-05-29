# ResourceHeartbeatDataDaemon

## Example Usage

```typescript
import { ResourceHeartbeatDataDaemon } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataDaemon = {
  data: {
    assignedMachines: 351239,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonName: "<value>",
    desiredMachines: 723101,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    healthyInstances: 920664,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    instances: [],
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