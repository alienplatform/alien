# DeploymentDetailResponseDomainsKubernetes

## Example Usage

```typescript
import { DeploymentDetailResponseDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                                   | [models.DeploymentDetailResponseTlsSecretRef](../models/deploymentdetailresponsetlssecretref.md) | :heavy_check_mark:                                                                               | Namespace-scoped Kubernetes TLS Secret reference.                                                |