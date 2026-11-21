# PlanDeploymentComputeCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { PlanDeploymentComputeCertificate } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeCertificate = {};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `aws`                                                    | *operations.PlanDeploymentComputeAwsUnion*               | :heavy_minus_sign:                                       | N/A                                                      |
| `azure`                                                  | *operations.PlanDeploymentComputeAzureUnion*             | :heavy_minus_sign:                                       | N/A                                                      |
| `gcp`                                                    | *operations.PlanDeploymentComputeGcpUnion*               | :heavy_minus_sign:                                       | N/A                                                      |
| `kubernetes`                                             | *operations.PlanDeploymentComputeDomainsKubernetesUnion* | :heavy_minus_sign:                                       | N/A                                                      |
| `managed`                                                | *operations.PlanDeploymentComputeManagedUnion*           | :heavy_minus_sign:                                       | N/A                                                      |