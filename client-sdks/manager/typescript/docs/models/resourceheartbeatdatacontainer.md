# ResourceHeartbeatDataContainer

## Example Usage

```typescript
import { ResourceHeartbeatDataContainer } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataContainer = {
  data: {
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    name: "<value>",
    namespace: "<value>",
    pods: [],
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