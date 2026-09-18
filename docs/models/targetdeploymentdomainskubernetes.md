# TargetDeploymentDomainsKubernetes

## Example Usage

```typescript
import { TargetDeploymentDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: TargetDeploymentDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                   | [models.TargetDeploymentTlsSecretRef](../models/targetdeploymenttlssecretref.md) | :heavy_check_mark:                                                               | Namespace-scoped Kubernetes TLS Secret reference.                                |