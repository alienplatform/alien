# ResourceHeartbeatDataAzureResourceGroup

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureResourceGroup } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureResourceGroup = {
  data: {
    managedTags: {
      "key": "<value>",
      "key1": "<value>",
      "key2": "<value>",
    },
    name: "<value>",
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "updating",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `data`                                                                                 | [models.AzureResourceGroupHeartbeatData](../models/azureresourcegroupheartbeatdata.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `resourceType`                                                                         | *"azure_resource_group"*                                                               | :heavy_check_mark:                                                                     | N/A                                                                                    |