# DataKubernetesSecret

## Example Usage

```typescript
import { DataKubernetesSecret } from "@alienplatform/platform-api/models";

let value: DataKubernetesSecret = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-12-05T01:04:58.637Z"),
      severity: "info",
    },
  ],
  namespace: "<value>",
  prefix: "<value>",
  secretMetadataListed: false,
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
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `events`                                                                         | [models.SyncReconcileRequestEvent34](../models/syncreconcilerequestevent34.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `prefix`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `secretMetadataListed`                                                           | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus34](../models/heartbeatstatus34.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetesSecret"*                                                             | :heavy_check_mark:                                                               | N/A                                                                              |