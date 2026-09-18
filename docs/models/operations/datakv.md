# DataKv

## Example Usage

```typescript
import { DataKv } from "@alienplatform/platform-api/models/operations";

let value: DataKv = {
  data: {
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "forbidden",
          severity: "info",
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

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion7* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"kv"*                  | :heavy_check_mark:      | N/A                     |