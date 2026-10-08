# DeploymentConfigDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentConfigDomainsCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentConfigDomainsCertificate = {};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `aws`                                            | *models.DeploymentConfigStackSettingsAwsUnion*   | :heavy_minus_sign:                               | N/A                                              |
| `azure`                                          | *models.DeploymentConfigStackSettingsAzureUnion* | :heavy_minus_sign:                               | N/A                                              |
| `gcp`                                            | *models.DeploymentConfigStackSettingsGcpUnion*   | :heavy_minus_sign:                               | N/A                                              |
| `kubernetes`                                     | *models.DeploymentConfigDomainsKubernetesUnion*  | :heavy_minus_sign:                               | N/A                                              |