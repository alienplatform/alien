# ManagerRetryResponseDomainsKubernetes1

## Example Usage

```typescript
import { ManagerRetryResponseDomainsKubernetes1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomainsKubernetes1 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                             | [models.ManagerRetryResponseTlsSecretRef1](../models/managerretryresponsetlssecretref1.md) | :heavy_check_mark:                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                          |