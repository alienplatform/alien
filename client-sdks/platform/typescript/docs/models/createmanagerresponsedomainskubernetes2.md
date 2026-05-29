# CreateManagerResponseDomainsKubernetes2

## Example Usage

```typescript
import { CreateManagerResponseDomainsKubernetes2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomainsKubernetes2 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                               | [models.CreateManagerResponseTlsSecretRef2](../models/createmanagerresponsetlssecretref2.md) | :heavy_check_mark:                                                                           | Namespace-scoped Kubernetes TLS Secret reference.                                            |