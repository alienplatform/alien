# DataPostgres

## Example Usage

```typescript
import { DataPostgres } from "@alienplatform/platform-api/models";

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

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `data`                                                           | *models.SyncReconcileRequestDataUnion8*                          | :heavy_check_mark:                                               | N/A                                                              |
| `resourceType`                                                   | [models.ResourceTypePostgres](../models/resourcetypepostgres.md) | :heavy_check_mark:                                               | N/A                                                              |