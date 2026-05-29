# DataNetwork

## Example Usage

```typescript
import { DataNetwork } from "@alienplatform/platform-api/models";

let value: DataNetwork = {
  data: {
    events: [],
    isByoVnet: false,
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "deleting",
      partial: false,
      stale: false,
    },
    backend: "azureVnet",
  },
  resourceType: "network",
};
```

## Fields

| Field                                    | Type                                     | Required                                 | Description                              |
| ---------------------------------------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| `data`                                   | *models.SyncReconcileRequestDataUnion10* | :heavy_check_mark:                       | N/A                                      |
| `resourceType`                           | *"network"*                              | :heavy_check_mark:                       | N/A                                      |