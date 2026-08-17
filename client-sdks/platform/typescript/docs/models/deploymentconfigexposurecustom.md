# DeploymentConfigExposureCustom

## Example Usage

```typescript
import { DeploymentConfigExposureCustom } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "ultimate-petal.net",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `certificate`                                                                | *models.DeploymentConfigCertificateUnion2*                                   | :heavy_check_mark:                                                           | Certificate publication or reference mode for Kubernetes public endpoints.   |
| `domain`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | Hostname routed by the Kubernetes public endpoint.                           |
| `mode`                                                                       | [models.DeploymentConfigModeCustom](../models/deploymentconfigmodecustom.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `route`                                                                      | *models.DeploymentConfigRouteUnion2*                                         | :heavy_check_mark:                                                           | Kubernetes route API selected for public endpoints.                          |