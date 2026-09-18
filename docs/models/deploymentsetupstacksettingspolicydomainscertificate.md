# DeploymentSetupStackSettingsPolicyDomainsCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyDomainsCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyDomainsCertificate = {};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `aws`                                                             | *models.DeploymentSetupStackSettingsPolicyAwsUnion*               | :heavy_minus_sign:                                                | N/A                                                               |
| `azure`                                                           | *models.DeploymentSetupStackSettingsPolicyAzureUnion*             | :heavy_minus_sign:                                                | N/A                                                               |
| `gcp`                                                             | *models.DeploymentSetupStackSettingsPolicyGcpUnion*               | :heavy_minus_sign:                                                | N/A                                                               |
| `kubernetes`                                                      | *models.DeploymentSetupStackSettingsPolicyDomainsKubernetesUnion* | :heavy_minus_sign:                                                | N/A                                                               |