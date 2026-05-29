# ResourceHeartbeatDataAzureResourceGroup

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureResourceGroup } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureResourceGroup = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-02-23T02:52:34.144Z"),
        severity: "info",
      },
    ],
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "deleted",
      partial: true,
      stale: false,
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