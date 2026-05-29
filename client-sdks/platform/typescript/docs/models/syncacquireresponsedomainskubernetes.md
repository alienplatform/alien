# SyncAcquireResponseDomainsKubernetes

## Example Usage

```typescript
import { SyncAcquireResponseDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                         | [models.SyncAcquireResponseTlsSecretRef](../models/syncacquireresponsetlssecretref.md) | :heavy_check_mark:                                                                     | Namespace-scoped Kubernetes TLS Secret reference.                                      |