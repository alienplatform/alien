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
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "deleting",
      partial: false,
      stale: false,
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