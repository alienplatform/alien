# DataKubernetesSecret

## Example Usage

```typescript
import { DataKubernetesSecret } from "@alienplatform/platform-api/models";

let value: DataKubernetesSecret = {
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
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: false,
  },
  backend: "kubernetesSecret",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `prefix`                                                                         | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `secretMetadataListed`                                                           | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SyncReconcileRequestStatus40](../models/syncreconcilerequeststatus40.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetesSecret"*                                                             | :heavy_check_mark:                                                               | N/A                                                                              |