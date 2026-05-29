# ResourceHeartbeatDataAzureStorageAccount

## Example Usage

```typescript
import { ResourceHeartbeatDataAzureStorageAccount } from "@alienplatform/manager-api/models";

let value: ResourceHeartbeatDataAzureStorageAccount = {
  data: {
    events: [],
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "updating",
      partial: true,
      stale: false,
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