# ManagerRetryResponseDomainsKubernetes3

## Example Usage

```typescript
import { ManagerRetryResponseDomainsKubernetes3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseDomainsKubernetes3 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                             | [models.ManagerRetryResponseTlsSecretRef3](../models/managerretryresponsetlssecretref3.md) | :heavy_check_mark:                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                          |