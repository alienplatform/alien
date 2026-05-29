# ResourceHeartbeatDataContainer

## Example Usage

```typescript
import { ResourceHeartbeatDataContainer } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataContainer = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    instances: [],
    name: "<value>",
    namespace: "<value>",
    replicas: {},
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    workloadKind: "daemonSet",
    backend: "kubernetes",
  },
  resourceType: "container",
};
```

## Fields

| Field                           | Type                            | Required                        | Description                     |
| ------------------------------- | ------------------------------- | ------------------------------- | ------------------------------- |
| `data`                          | *models.ContainerHeartbeatData* | :heavy_check_mark:              | N/A                             |
| `resourceType`                  | *"container"*                   | :heavy_check_mark:              | N/A                             |