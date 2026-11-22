# DeploymentExposureGenerated

## Example Usage

```typescript
import { DeploymentExposureGenerated } from "@alienplatform/platform-api/models";

let value: DeploymentExposureGenerated = {
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

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.DeploymentCertificateUnion1*                                       | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [models.DeploymentModeGenerated](../models/deploymentmodegenerated.md)     | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.DeploymentRouteUnion1*                                             | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |