# DataAzureStorageAccount

## Example Usage

```typescript
import { DataAzureStorageAccount } from "@alienplatform/platform-api/models";

let value: DataAzureStorageAccount = {
  data: {
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "unhealthy",
      lifecycle: "unknown",
      partial: true,
      stale: false,
    },
  },
  resourceType: "azure_storage_account",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `data`                                                                     | [models.SyncReconcileRequestData3](../models/syncreconcilerequestdata3.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `resourceType`                                                             | *"azure_storage_account"*                                                  | :heavy_check_mark:                                                         | N/A                                                                        |