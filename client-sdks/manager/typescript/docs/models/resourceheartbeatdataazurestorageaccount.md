# ResourceHeartbeatDataAzureStorageAccount

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureStorageAccount } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureStorageAccount = {
  data: {
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "stopping",
      partial: true,
      stale: true,
    },
  },
  resourceType: "azure_storage_account",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `data`                                                                                   | [models.AzureStorageAccountHeartbeatData](../models/azurestorageaccountheartbeatdata.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `resourceType`                                                                           | *"azure_storage_account"*                                                                | :heavy_check_mark:                                                                       | N/A                                                                                      |