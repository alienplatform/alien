# DeploymentDetailResponseDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentDetailResponseDomainsCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseDomainsCertificate = {};
```

## Fields

| Field                                                   | Type                                                    | Required                                                | Description                                             |
| ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- | ------------------------------------------------------- |
| `aws`                                                   | *models.DeploymentDetailResponseAwsUnion*               | :heavy_minus_sign:                                      | N/A                                                     |
| `azure`                                                 | *models.DeploymentDetailResponseAzureUnion*             | :heavy_minus_sign:                                      | N/A                                                     |
| `gcp`                                                   | *models.DeploymentDetailResponseGcpUnion*               | :heavy_minus_sign:                                      | N/A                                                     |
| `kubernetes`                                            | *models.DeploymentDetailResponseDomainsKubernetesUnion* | :heavy_minus_sign:                                      | N/A                                                     |