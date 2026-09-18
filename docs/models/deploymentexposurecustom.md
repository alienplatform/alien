# DeploymentExposureCustom

## Example Usage

```typescript
import { DeploymentExposureCustom } from "@alienplatform/platform-api/models";

let value: DeploymentExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "bulky-toothpick.net",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.DeploymentCertificateUnion2*                                       | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | [models.DeploymentModeCustom](../models/deploymentmodecustom.md)           | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.DeploymentRouteUnion2*                                             | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |