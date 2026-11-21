# NewDeploymentRequestExposureGenerated

## Example Usage

```typescript
import { NewDeploymentRequestExposureGenerated } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 134192,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `certificate`                                                                              | *models.NewDeploymentRequestCertificateUnion1*                                             | :heavy_check_mark:                                                                         | Certificate publication or reference mode for Kubernetes public endpoints.                 |
| `mode`                                                                                     | [models.NewDeploymentRequestModeGenerated](../models/newdeploymentrequestmodegenerated.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
| `route`                                                                                    | *models.NewDeploymentRequestRouteUnion1*                                                   | :heavy_check_mark:                                                                         | Kubernetes route API selected for public endpoints.                                        |