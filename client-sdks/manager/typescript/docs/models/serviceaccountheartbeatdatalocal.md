# ServiceAccountHeartbeatDataLocal

## Example Usage

```typescript
import { ServiceAccountHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ServiceAccountHeartbeatDataLocal = {
  configured: false,
  identity: "<value>",
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
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `configured`                                                                       | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `identity`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ServiceAccountHeartbeatStatus](../models/serviceaccountheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"local"*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |