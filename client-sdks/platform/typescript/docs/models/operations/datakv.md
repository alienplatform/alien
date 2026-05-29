# DataKv

## Example Usage

```typescript
import { DataKv } from "@alienplatform/platform-api/models/operations";

let value: DataKv = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-06-08T10:49:40.534Z"),
        severity: "warning",
      },
    ],
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
      lifecycle: "failed",
      partial: true,
      stale: true,
    },
    storageAccountName: "<value>",
    tableExists: true,
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