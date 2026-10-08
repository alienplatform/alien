# CreateManagerResponseDomainsKubernetes3

## Example Usage

```typescript
import { CreateManagerResponseDomainsKubernetes3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseDomainsKubernetes3 = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                               | [models.CreateManagerResponseTlsSecretRef3](../models/createmanagerresponsetlssecretref3.md) | :heavy_check_mark:                                                                           | Namespace-scoped Kubernetes TLS Secret reference.                                            |