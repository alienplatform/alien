# ServiceAccountHeartbeatDataLocal

## Example Usage

```typescript
import { ServiceAccountHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: ServiceAccountHeartbeatDataLocal = {
  configured: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  identity: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `configured`                                                                       | *boolean*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |
| `events`                                                                           | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `identity`                                                                         | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ServiceAccountHeartbeatStatus](../models/serviceaccountheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"local"*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |