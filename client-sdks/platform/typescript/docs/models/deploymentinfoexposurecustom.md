# DeploymentInfoExposureCustom

## Example Usage

```typescript
import { DeploymentInfoExposureCustom } from "@alienplatform/platform-api/models";

let value: DeploymentInfoExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "ultimate-cap.org",
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
| `certificate`                                                              | *models.DeploymentInfoCertificateUnion2*                                   | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | [models.SetupUpdateModeCustom](../models/setupupdatemodecustom.md)         | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.DeploymentInfoRouteUnion2*                                         | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |