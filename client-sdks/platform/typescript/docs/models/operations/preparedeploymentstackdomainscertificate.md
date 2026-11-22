# PrepareDeploymentStackDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { PrepareDeploymentStackDomainsCertificate } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackDomainsCertificate = {};
```

## Fields

| Field                                                     | Type                                                      | Required                                                  | Description                                               |
| --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- | --------------------------------------------------------- |
| `aws`                                                     | *operations.PrepareDeploymentStackAwsUnion*               | :heavy_minus_sign:                                        | N/A                                                       |
| `azure`                                                   | *operations.PrepareDeploymentStackAzureUnion*             | :heavy_minus_sign:                                        | N/A                                                       |
| `gcp`                                                     | *operations.PrepareDeploymentStackGcpUnion*               | :heavy_minus_sign:                                        | N/A                                                       |
| `kubernetes`                                              | *operations.PrepareDeploymentStackDomainsKubernetesUnion* | :heavy_minus_sign:                                        | N/A                                                       |