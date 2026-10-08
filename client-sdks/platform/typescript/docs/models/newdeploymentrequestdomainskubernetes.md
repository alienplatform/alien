# NewDeploymentRequestDomainsKubernetes

## Example Usage

```typescript
import { NewDeploymentRequestDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                           | [models.NewDeploymentRequestTlsSecretRef](../models/newdeploymentrequesttlssecretref.md) | :heavy_check_mark:                                                                       | Namespace-scoped Kubernetes TLS Secret reference.                                        |