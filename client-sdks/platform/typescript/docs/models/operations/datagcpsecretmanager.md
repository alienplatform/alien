# DataGcpSecretManager

## Example Usage

```typescript
import { DataGcpSecretManager } from "@alienplatform/platform-api/models/operations";

let value: DataGcpSecretManager = {
  events: [],
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "gcpSecretManager",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent32](../../models/operations/getrawresourceheartbeatevent32.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `location`                                                                                               | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `prefix`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `projectId`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `secretMetadataListed`                                                                                   | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus32](../../models/operations/datastatus32.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"gcpSecretManager"*                                                                                     | :heavy_check_mark:                                                                                       | N/A                                                                                                      |