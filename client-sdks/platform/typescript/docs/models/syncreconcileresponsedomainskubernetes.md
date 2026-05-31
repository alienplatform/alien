# SyncReconcileResponseDomainsKubernetes

## Example Usage

```typescript
import { SyncReconcileResponseDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                             | [models.SyncReconcileResponseTlsSecretRef](../models/syncreconcileresponsetlssecretref.md) | :heavy_check_mark:                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                          |