# DataGcpSecretManager

## Example Usage

```typescript
import { DataGcpSecretManager } from "@alienplatform/platform-api/models";

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
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  backend: "gcpSecretManager",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent32](../models/syncreconcilerequestevent32.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `location`                                                                       | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `prefix`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `projectId`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `secretMetadataListed`                                                           | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus32](../models/heartbeatstatus32.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcpSecretManager"*                                                             | :heavy_check_mark:                                                               | N/A                                                                              |