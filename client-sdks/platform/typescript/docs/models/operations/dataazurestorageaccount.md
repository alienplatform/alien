# DataAzureStorageAccount

## Example Usage

```typescript
import { DataAzureStorageAccount } from "@alienplatform/platform-api/models/operations";

let value: DataAzureStorageAccount = {
  data: {
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_storage_account",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `data`                                                                                                     | [operations.GetResourceDeploymentDetailData3](../../models/operations/getresourcedeploymentdetaildata3.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `resourceType`                                                                                             | *"azure_storage_account"*                                                                                  | :heavy_check_mark:                                                                                         | N/A                                                                                                        |