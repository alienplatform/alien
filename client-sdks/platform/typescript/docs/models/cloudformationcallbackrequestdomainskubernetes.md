# CloudFormationCallbackRequestDomainsKubernetes

## Example Usage

```typescript
import { CloudFormationCallbackRequestDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                                             | [models.CloudFormationCallbackRequestTlsSecretRef](../models/cloudformationcallbackrequesttlssecretref.md) | :heavy_check_mark:                                                                                         | Namespace-scoped Kubernetes TLS Secret reference.                                                          |