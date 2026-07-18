# SyncAcquireResponseDeploymentDomainsKubernetes

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                                             | [models.SyncAcquireResponseDeploymentTlsSecretRef](../models/syncacquireresponsedeploymenttlssecretref.md) | :heavy_check_mark:                                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                                          |