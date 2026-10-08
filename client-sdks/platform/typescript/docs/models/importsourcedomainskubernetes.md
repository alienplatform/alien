# ImportSourceDomainsKubernetes

## Example Usage

```typescript
import { ImportSourceDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: ImportSourceDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `tlsSecretRef`                                                           | [models.ImportSourceTlsSecretRef](../models/importsourcetlssecretref.md) | :heavy_check_mark:                                                       | Namespace-scoped Kubernetes TLS Secret reference.                        |