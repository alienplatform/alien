# CreateManagerResponseDomainsKubernetes1

## Example Usage

```typescript
import { CreateManagerResponseDomainsKubernetes1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomainsKubernetes1 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                               | [models.CreateManagerResponseTlsSecretRef1](../models/createmanagerresponsetlssecretref1.md) | :heavy_check_mark:                                                                           | Namespace-scoped Kubernetes TLS Secret reference.                                            |