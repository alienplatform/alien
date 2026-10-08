# DeploymentSetupStackSettingsPolicyExposureCustom

## Example Usage

```typescript
import { DeploymentSetupStackSettingsPolicyExposureCustom } from "@alienplatform/platform-api/models";

let value: DeploymentSetupStackSettingsPolicyExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "round-fog.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 53529,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                    | *models.DeploymentSetupStackSettingsPolicyCertificateUnion2*                                                     | :heavy_check_mark:                                                                                               | Certificate publication or reference mode for Kubernetes public endpoints.                                       |
| `domain`                                                                                                         | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Hostname routed by the Kubernetes public endpoint.                                                               |
| `mode`                                                                                                           | [models.DeploymentSetupStackSettingsPolicyModeCustom](../models/deploymentsetupstacksettingspolicymodecustom.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `route`                                                                                                          | *models.DeploymentSetupStackSettingsPolicyRouteUnion2*                                                           | :heavy_check_mark:                                                                                               | Kubernetes route API selected for public endpoints.                                                              |