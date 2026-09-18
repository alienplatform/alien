# ManagerRetryResponseDomainsKubernetes2

## Example Usage

```typescript
import { ManagerRetryResponseDomainsKubernetes2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomainsKubernetes2 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                             | [models.ManagerRetryResponseTlsSecretRef2](../models/managerretryresponsetlssecretref2.md) | :heavy_check_mark:                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                          |