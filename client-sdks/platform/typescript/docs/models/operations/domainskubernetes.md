# DomainsKubernetes

## Example Usage

```typescript
import { DomainsKubernetes } from "@alienplatform/platform-api/models/operations";

let value: DomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `tlsSecretRef`                                                     | [operations.TlsSecretRef](../../models/operations/tlssecretref.md) | :heavy_check_mark:                                                 | Namespace-scoped Kubernetes TLS Secret reference.                  |