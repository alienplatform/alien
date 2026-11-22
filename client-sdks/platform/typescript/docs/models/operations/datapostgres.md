# DataPostgres

## Example Usage

```typescript
import { DataPostgres } from "@alienplatform/platform-api/models/operations";

let value: DataPostgres = {
  data: {
    serverName: "<value>",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "stopped",
      partial: true,
      stale: false,
    },
    backend: "flexibleServer",
  },
  resourceType: "postgres",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion8* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"postgres"*            | :heavy_check_mark:      | N/A                     |