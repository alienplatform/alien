# DataAzureResourceGroup

## Example Usage

```typescript
import { DataAzureResourceGroup } from "@alienplatform/platform-api/models/operations";

let value: DataAzureResourceGroup = {
  data: {
    managedTags: {},
    name: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "collection-failed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "unknown",
      partial: false,
      stale: true,
    },
  },
  resourceType: "azure_resource_group",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                     | [operations.GetResourceDeploymentDetailData2](../../models/operations/getresourcedeploymentdetaildata2.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `resourceType`                                                                                             | [operations.ResourceTypeAzureResourceGroup](../../models/operations/resourcetypeazureresourcegroup.md)     | :heavy_check_mark:                                                                                         | N/A                                                                                                        |