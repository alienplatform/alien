# DataKv

## Example Usage

```typescript
import { DataKv } from "@alienplatform/platform-api/models";

let value: DataKv = {
  data: {
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "api-unavailable",
          severity: "error",
          source: "<value>",
        },
      ],
      health: "healthy",
      lifecycle: "scaling",
      partial: false,
      stale: true,
    },
    storageAccountName: "<value>",
    tableExists: false,
    tableName: "<value>",
    backend: "azureTable",
  },
  resourceType: "kv",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion7* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"kv"*                                  | :heavy_check_mark:                      | N/A                                     |