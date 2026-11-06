# PrepareDeploymentStackCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { PrepareDeploymentStackCertificate } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackCertificate = {};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `aws`                                                     | *operations.PrepareDeploymentStackAwsUnion*               | :heavy_minus_sign:                                        | N/A                                                       |
| `azure`                                                   | *operations.PrepareDeploymentStackAzureUnion*             | :heavy_minus_sign:                                        | N/A                                                       |
| `gcp`                                                     | *operations.PrepareDeploymentStackGcpUnion*               | :heavy_minus_sign:                                        | N/A                                                       |
| `kubernetes`                                              | *operations.PrepareDeploymentStackDomainsKubernetesUnion* | :heavy_minus_sign:                                        | N/A                                                       |
| `managed`                                                 | *operations.PrepareDeploymentStackManagedUnion*           | :heavy_minus_sign:                                        | N/A                                                       |