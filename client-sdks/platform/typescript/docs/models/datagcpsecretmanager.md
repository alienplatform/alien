# DataGcpSecretManager

## Example Usage

```typescript
import { DataGcpSecretManager } from "@alienplatform/platform-api/models";

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

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `location`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `prefix`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `projectId`                                                                | *string*                                                                   | :heavy_check_mark:                                                         | N/A                                                                        |
| `secretMetadataListed`                                                     | *boolean*                                                                  | :heavy_check_mark:                                                         | N/A                                                                        |
| `status`                                                                   | [models.ResourceHeartbeatStatus36](../models/resourceheartbeatstatus36.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `backend`                                                                  | *"gcpSecretManager"*                                                       | :heavy_check_mark:                                                         | N/A                                                                        |