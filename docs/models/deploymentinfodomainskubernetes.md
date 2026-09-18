# DeploymentInfoDomainsKubernetes

## Example Usage

```typescript
import { DeploymentInfoDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentInfoDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `tlsSecretRef`                                                               | [models.DeploymentInfoTlsSecretRef](../models/deploymentinfotlssecretref.md) | :heavy_check_mark:                                                           | Namespace-scoped Kubernetes TLS Secret reference.                            |