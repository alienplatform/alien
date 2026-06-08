# CreateSetupRegistrationOperationRequestDomainsKubernetes

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestDomainsKubernetes } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                                                                 | [models.CreateSetupRegistrationOperationRequestTlsSecretRef](../models/createsetupregistrationoperationrequesttlssecretref.md) | :heavy_check_mark:                                                                                                             | Namespace-scoped Kubernetes TLS Secret reference.                                                                              |