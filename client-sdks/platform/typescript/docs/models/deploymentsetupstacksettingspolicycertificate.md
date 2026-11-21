# DeploymentSetupStackSettingsPolicyCertificate

Platform-specific certificate references for custom domains.

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyCertificate } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyCertificate = {};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `aws`                                                             | *models.DeploymentSetupStackSettingsPolicyAwsUnion*               | :heavy_minus_sign:                                                | N/A                                                               |
| `azure`                                                           | *models.DeploymentSetupStackSettingsPolicyAzureUnion*             | :heavy_minus_sign:                                                | N/A                                                               |
| `gcp`                                                             | *models.DeploymentSetupStackSettingsPolicyGcpUnion*               | :heavy_minus_sign:                                                | N/A                                                               |
| `kubernetes`                                                      | *models.DeploymentSetupStackSettingsPolicyDomainsKubernetesUnion* | :heavy_minus_sign:                                                | N/A                                                               |
| `managed`                                                         | *models.DeploymentSetupStackSettingsPolicyManagedUnion*           | :heavy_minus_sign:                                                | N/A                                                               |