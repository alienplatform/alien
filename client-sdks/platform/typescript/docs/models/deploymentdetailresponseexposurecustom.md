# DeploymentDetailResponseExposureCustom

## Example Usage

```typescript
import { DeploymentDetailResponseExposureCustom } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "flawed-daughter.net",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | *models.DeploymentDetailResponseCertificateUnion2*                                           | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `domain`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Hostname routed by the Kubernetes public endpoint.                                           |
| `mode`                                                                                       | [models.DeploymentDetailResponseModeCustom](../models/deploymentdetailresponsemodecustom.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.DeploymentDetailResponseRouteUnion2*                                                 | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |