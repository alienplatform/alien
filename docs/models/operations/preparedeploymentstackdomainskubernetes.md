# PrepareDeploymentStackDomainsKubernetes

## Example Usage

```typescript
import { PrepareDeploymentStackDomainsKubernetes } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `tlsSecretRef`                                                                                                 | [operations.PrepareDeploymentStackTlsSecretRef](../../models/operations/preparedeploymentstacktlssecretref.md) | :heavy_check_mark:                                                                                             | Namespace-scoped Kubernetes TLS Secret reference.                                                              |