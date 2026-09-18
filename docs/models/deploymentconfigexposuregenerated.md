# DeploymentConfigExposureGenerated

## Example Usage

```typescript
import { DeploymentConfigExposureGenerated } from "@alienplatform/platform-api/models";

let value: DeploymentConfigExposureGenerated = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `certificate`                                                                      | *models.DeploymentConfigCertificateUnion1*                                         | :heavy_check_mark:                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.         |
| `mode`                                                                             | [models.DeploymentConfigModeGenerated](../models/deploymentconfigmodegenerated.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `route`                                                                            | *models.DeploymentConfigRouteUnion1*                                               | :heavy_check_mark:                                                                 | Kubernetes route API selected for public endpoints.                                |