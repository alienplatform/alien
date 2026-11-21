# TargetDeploymentExposureCustom

## Example Usage

```typescript
import { TargetDeploymentExposureCustom } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "grounded-ignorance.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 97581,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `certificate`                                                                | *models.TargetDeploymentCertificateUnion2*                                   | :heavy_check_mark:                                                           | Certificate publication or reference mode for Kubernetes public endpoints.   |
| `domain`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | Hostname routed by the Kubernetes public endpoint.                           |
| `mode`                                                                       | [models.TargetDeploymentModeCustom](../models/targetdeploymentmodecustom.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `route`                                                                      | *models.TargetDeploymentRouteUnion2*                                         | :heavy_check_mark:                                                           | Kubernetes route API selected for public endpoints.                          |