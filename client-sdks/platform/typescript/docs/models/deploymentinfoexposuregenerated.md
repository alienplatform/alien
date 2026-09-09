# DeploymentInfoExposureGenerated

## Example Usage

```typescript
import { DeploymentInfoExposureGenerated } from "@alienplatform/platform-api/models";

let value: DeploymentInfoExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 316835,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.DeploymentInfoCertificateUnion1*                                   | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [models.SetupUpdateModeGenerated](../models/setupupdatemodegenerated.md)   | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.DeploymentInfoRouteUnion1*                                         | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |