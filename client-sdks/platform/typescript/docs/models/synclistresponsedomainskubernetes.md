# SyncListResponseDomainsKubernetes

## Example Usage

```typescript
import { SyncListResponseDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: SyncListResponseDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                   | [models.SyncListResponseTlsSecretRef](../models/synclistresponsetlssecretref.md) | :heavy_check_mark:                                                               | Namespace-scoped Kubernetes TLS Secret reference.                                |