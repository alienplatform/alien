# DeploymentConfigDomainsKubernetes

## Example Usage

```typescript
import { DeploymentConfigDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                   | [models.DeploymentConfigTlsSecretRef](../models/deploymentconfigtlssecretref.md) | :heavy_check_mark:                                                               | Namespace-scoped Kubernetes TLS Secret reference.                                |