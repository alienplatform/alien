# DataKubernetesPods

Kubernetes: pods carrying the sandbox label, in the deployment's namespace.

## Example Usage

```typescript
import { DataKubernetesPods } from "@alienplatform/platform-api/models/operations";

let value: DataKubernetesPods = {
  activeSessions: 284392,
  idlePods: 443425,
  namespace: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: false,
    stale: true,
  },
  backend: "kubernetesPods",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `activeSessions`                                                   | *number*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `idlePods`                                                         | *number*                                                           | :heavy_check_mark:                                                 | Claimed but unused pods waiting in the pool.                       |
| `namespace`                                                        | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus75](../../models/operations/datastatus75.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `backend`                                                          | *"kubernetesPods"*                                                 | :heavy_check_mark:                                                 | N/A                                                                |