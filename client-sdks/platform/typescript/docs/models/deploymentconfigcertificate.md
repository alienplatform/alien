# DeploymentConfigCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentConfigCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentConfigCertificate = {};
```

## Fields

| Field                                            | Type                                             | Required                                         | Description                                      |
| ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ | ------------------------------------------------ |
| `aws`                                            | *models.DeploymentConfigStackSettingsAwsUnion*   | :heavy_minus_sign:                               | N/A                                              |
| `azure`                                          | *models.DeploymentConfigStackSettingsAzureUnion* | :heavy_minus_sign:                               | N/A                                              |
| `gcp`                                            | *models.DeploymentConfigStackSettingsGcpUnion*   | :heavy_minus_sign:                               | N/A                                              |
| `kubernetes`                                     | *models.DeploymentConfigDomainsKubernetesUnion*  | :heavy_minus_sign:                               | N/A                                              |
| `managed`                                        | *models.DeploymentConfigManagedUnion*            | :heavy_minus_sign:                               | N/A                                              |