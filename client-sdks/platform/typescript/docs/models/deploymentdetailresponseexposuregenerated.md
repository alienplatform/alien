# DeploymentDetailResponseExposureGenerated

## Example Usage

```typescript
import { DeploymentDetailResponseExposureGenerated } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 152308,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                      | *models.DeploymentDetailResponseCertificateUnion1*                                                 | :heavy_check_mark:                                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.                         |
| `mode`                                                                                             | [models.DeploymentDetailResponseModeGenerated](../models/deploymentdetailresponsemodegenerated.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `route`                                                                                            | *models.DeploymentDetailResponseRouteUnion1*                                                       | :heavy_check_mark:                                                                                 | Kubernetes route API selected for public endpoints.                                                |