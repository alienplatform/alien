# VaultHeartbeatDataGcpSecretManager

## Example Usage

```typescript
import { VaultHeartbeatDataGcpSecretManager } from "@alienplatform/manager-api/models";

let value: VaultHeartbeatDataGcpSecretManager = {
  location: "<value>",
  prefix: "<value>",
  projectId: "<id>",
  secretMetadataListed: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "gcpSecretManager",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `location`                                                       | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `prefix`                                                         | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `projectId`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `secretMetadataListed`                                           | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.VaultHeartbeatStatus](../models/vaultheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"gcpSecretManager"*                                             | :heavy_check_mark:                                               | N/A                                                              |