# DataKubernetesSecret

## Example Usage

```typescript
import { DataKubernetesSecret } from "@alienplatform/platform-api/models/operations";

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
        severity: "error",
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

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent34](../../models/operations/getrawresourceheartbeatevent34.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `namespace`                                                                                              | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `prefix`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `secretMetadataListed`                                                                                   | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus34](../../models/operations/datastatus34.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"kubernetesSecret"*                                                                                     | :heavy_check_mark:                                                                                       | N/A                                                                                                      |