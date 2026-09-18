# PlanDeploymentComputeDomainsKubernetes

## Example Usage

```typescript
import { PlanDeploymentComputeDomainsKubernetes } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeDomainsKubernetes = {
  tlsSecretRef: {
    secretName: "<value>",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `tlsSecretRef`                                                                                               | [operations.PlanDeploymentComputeTlsSecretRef](../../models/operations/plandeploymentcomputetlssecretref.md) | :heavy_check_mark:                                                                                           | Namespace-scoped Kubernetes TLS Secret reference.                                                            |