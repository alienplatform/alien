# VaultHeartbeatDataKubernetesSecret

## Example Usage

```typescript
import { VaultHeartbeatDataKubernetesSecret } from "@alienplatform/manager-api/models";

let value: VaultHeartbeatDataKubernetesSecret = {
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: true,
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
  backend: "kubernetesSecret",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `namespace`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `prefix`                                                         | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `secretMetadataListed`                                           | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.VaultHeartbeatStatus](../models/vaultheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"kubernetesSecret"*                                             | :heavy_check_mark:                                               | N/A                                                              |