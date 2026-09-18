# DataNetwork

## Example Usage

```typescript
import { DataNetwork } from "@alienplatform/platform-api/models";

let value: DataNetwork = {
  data: {
    isByoVnet: true,
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "unknown",
      partial: true,
      stale: true,
    },
    backend: "azureVnet",
  },
  resourceType: "network",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion11* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"network"*                              | :heavy_check_mark:                       | N/A                                      |