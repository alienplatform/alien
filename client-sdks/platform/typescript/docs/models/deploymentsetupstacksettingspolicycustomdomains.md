# DeploymentSetupStackSettingsPolicyCustomDomains

Custom domain configuration for a single resource.

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyCustomDomains } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyCustomDomains = {
  certificate: {},
  domain: "tough-shadowbox.info",
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                                    | [models.DeploymentSetupStackSettingsPolicyDomainsCertificate](../models/deploymentsetupstacksettingspolicydomainscertificate.md) | :heavy_check_mark:                                                                                                               | Platform-specific certificate references for custom domains.                                                                     |
| `domain`                                                                                                                         | *string*                                                                                                                         | :heavy_check_mark:                                                                                                               | Fully qualified domain name to use.                                                                                              |