# TargetDeploymentExposureGenerated

## Example Usage

```typescript
import { TargetDeploymentExposureGenerated } from "@alienplatform/platform-api/models";

let value: TargetDeploymentExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 396903,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `certificate`                                                                      | *models.TargetDeploymentCertificateUnion1*                                         | :heavy_check_mark:                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.         |
| `mode`                                                                             | [models.TargetDeploymentModeGenerated](../models/targetdeploymentmodegenerated.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `route`                                                                            | *models.TargetDeploymentRouteUnion1*                                               | :heavy_check_mark:                                                                 | Kubernetes route API selected for public endpoints.                                |