# NewDeploymentRequestDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { NewDeploymentRequestDomainsCertificate } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestDomainsCertificate = {};
```

## Fields

| Field                                               | Type                                                | Required                                            | Description                                         |
| --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- | --------------------------------------------------- |
| `aws`                                               | *models.NewDeploymentRequestAwsUnion*               | :heavy_minus_sign:                                  | N/A                                                 |
| `azure`                                             | *models.NewDeploymentRequestAzureUnion*             | :heavy_minus_sign:                                  | N/A                                                 |
| `gcp`                                               | *models.NewDeploymentRequestGcpUnion*               | :heavy_minus_sign:                                  | N/A                                                 |
| `kubernetes`                                        | *models.NewDeploymentRequestDomainsKubernetesUnion* | :heavy_minus_sign:                                  | N/A                                                 |