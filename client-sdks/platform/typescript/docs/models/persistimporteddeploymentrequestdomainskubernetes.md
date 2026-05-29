# PersistImportedDeploymentRequestDomainsKubernetes

## Example Usage

```typescript
import { PersistImportedDeploymentRequestDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                                                   | [models.PersistImportedDeploymentRequestTlsSecretRef](../models/persistimporteddeploymentrequesttlssecretref.md) | :heavy_check_mark:                                                                                               | Namespace-scoped Kubernetes TLS Secret reference.                                                                |