# DeploymentDomainsKubernetes

## Example Usage

```typescript
import { DeploymentDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `tlsSecretRef`                                                       | [models.DeploymentTlsSecretRef](../models/deploymenttlssecretref.md) | :heavy_check_mark:                                                   | Namespace-scoped Kubernetes TLS Secret reference.                    |