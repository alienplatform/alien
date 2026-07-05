# DataGcpSecretManager

## Example Usage

```typescript
import { DataGcpSecretManager } from "@alienplatform/platform-api/models/operations";

let value: DataGcpSecretManager = {
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "gcpSecretManager",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `location`                                                         | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `prefix`                                                           | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `projectId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `secretMetadataListed`                                             | *boolean*                                                          | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus36](../../models/operations/datastatus36.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"gcpSecretManager"*                                               | :heavy_check_mark:                                                 | N/A                                                                |