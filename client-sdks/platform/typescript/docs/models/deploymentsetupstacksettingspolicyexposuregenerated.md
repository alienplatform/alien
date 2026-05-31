# DeploymentSetupStackSettingsPolicyExposureGenerated

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyExposureGenerated } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                                  | Type                                                                                                                   | Required                                                                                                               | Description                                                                                                            |
| ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                          | *models.DeploymentSetupStackSettingsPolicyCertificateUnion1*                                                           | :heavy_check_mark:                                                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.                                             |
| `mode`                                                                                                                 | [models.DeploymentSetupStackSettingsPolicyModeGenerated](../models/deploymentsetupstacksettingspolicymodegenerated.md) | :heavy_check_mark:                                                                                                     | N/A                                                                                                                    |
| `route`                                                                                                                | *models.DeploymentSetupStackSettingsPolicyRouteUnion1*                                                                 | :heavy_check_mark:                                                                                                     | Kubernetes route API selected for public endpoints.                                                                    |