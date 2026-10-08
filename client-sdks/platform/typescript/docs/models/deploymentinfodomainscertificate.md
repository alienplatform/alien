# DeploymentInfoDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentInfoDomainsCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentInfoDomainsCertificate = {};
```

## Fields

| Field                                         | Type                                          | Required                                      | Description                                   |
| --------------------------------------------- | --------------------------------------------- | --------------------------------------------- | --------------------------------------------- |
| `aws`                                         | *models.DeploymentInfoAwsUnion*               | :heavy_minus_sign:                            | N/A                                           |
| `azure`                                       | *models.DeploymentInfoAzureUnion*             | :heavy_minus_sign:                            | N/A                                           |
| `gcp`                                         | *models.DeploymentInfoGcpUnion*               | :heavy_minus_sign:                            | N/A                                           |
| `kubernetes`                                  | *models.DeploymentInfoDomainsKubernetesUnion* | :heavy_minus_sign:                            | N/A                                           |