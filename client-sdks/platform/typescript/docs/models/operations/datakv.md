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
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "degraded",
      lifecycle: "unknown",
      partial: true,
      stale: false,
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