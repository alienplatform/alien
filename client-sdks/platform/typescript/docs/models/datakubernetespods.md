# DataKubernetesPods

Kubernetes: pods carrying the sandbox label, in the deployment's namespace.

## Example Usage

```typescript
import { DataKubernetesPods } from "@alienplatform/platform-api/models";

let value: DataKubernetesPods = {
  activeSessions: 284392,
  idlePods: 443425,
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "kubernetesPods",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `activeSessions`                                                                 | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `idlePods`                                                                       | *number*                                                                         | :heavy_check_mark:                                                               | Claimed but unused pods waiting in the pool.                                     |
| `namespace`                                                                      | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.SyncReconcileRequestStatus76](../models/syncreconcilerequeststatus76.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"kubernetesPods"*                                                               | :heavy_check_mark:                                                               | N/A                                                                              |