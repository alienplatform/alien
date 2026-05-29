# DataAzureStorageAccount

## Example Usage

```typescript
import { DataAzureStorageAccount } from "@alienplatform/platform-api/models";

let value: DataAzureStorageAccount = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2024-05-16T02:06:58.117Z"),
        severity: "warning",
      },
    ],
    name: "<value>",
    primaryEndpoints: {},
    secondaryEndpoints: {},
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "info",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "creating",
      partial: false,
      stale: true,
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