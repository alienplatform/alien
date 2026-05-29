# ResourceHeartbeatDataNetwork

## Example Usage

```typescript
import { ResourceHeartbeatDataNetwork } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataNetwork = {
  data: {
    isByoVpc: true,
    status: {
      collectionIssues: [],
      health: "healthy",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
    backend: "gcpVpc",
  },
  resourceType: "network",
};
```

## Fields

| Field                         | Type                          | Required                      | Description                   |
| ----------------------------- | ----------------------------- | ----------------------------- | ----------------------------- |
| `data`                        | *models.NetworkHeartbeatData* | :heavy_check_mark:            | N/A                           |
| `resourceType`                | *"network"*                   | :heavy_check_mark:            | N/A                           |