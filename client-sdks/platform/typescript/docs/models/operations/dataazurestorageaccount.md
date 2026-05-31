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
      health: "unhealthy",
      lifecycle: "stopped",
      partial: false,
      stale: false,
    },
  },
  resourceType: "azure_storage_account",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `data`                                               | [operations.Data3](../../models/operations/data3.md) | :heavy_check_mark:                                   | N/A                                                  |
| `resourceType`                                       | *"azure_storage_account"*                            | :heavy_check_mark:                                   | N/A                                                  |