# VaultHeartbeatDataKubernetesSecret

## Example Usage

```typescript
import { VaultHeartbeatDataKubernetesSecret } from "@alienplatform/manager-api/models";

let value: VaultHeartbeatDataKubernetesSecret = {
  events: [],
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "kubernetesSecret",
};
```

## Fields

| Field                                                            | Type                                                             | Required                                                         | Description                                                      |
| ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------------------- |
| `events`                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]           | :heavy_check_mark:                                               | N/A                                                              |
| `namespace`                                                      | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `prefix`                                                         | *string*                                                         | :heavy_check_mark:                                               | N/A                                                              |
| `secretMetadataListed`                                           | *boolean*                                                        | :heavy_check_mark:                                               | N/A                                                              |
| `status`                                                         | [models.VaultHeartbeatStatus](../models/vaultheartbeatstatus.md) | :heavy_check_mark:                                               | N/A                                                              |
| `backend`                                                        | *"kubernetesSecret"*                                             | :heavy_check_mark:                                               | N/A                                                              |