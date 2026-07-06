# DataAzureResourceGroup

## Example Usage

```typescript
import { DataAzureResourceGroup } from "@alienplatform/platform-api/models";

let value: DataAzureResourceGroup = {
  data: {
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "timed-out",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "deleting",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData2](../models/syncreconcilerequestdata2.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceType`                                                             | *"azure_resource_group"*                                                   | :heavy_check_mark:                                                         | N/A                                                                        |