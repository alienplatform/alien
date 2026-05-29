# KubernetesCustomCertificateConfig

## Example Usage

```typescript
import { KubernetesCustomCertificateConfig } from "@alienplatform/manager-api/models";

let value: KubernetesCustomCertificateConfig = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `tlsSecretRef`                                                       | [models.KubernetesTlsSecretRef](../models/kubernetestlssecretref.md) | :heavy_check_mark:                                                   | Namespace-scoped Kubernetes TLS Secret reference.                    |