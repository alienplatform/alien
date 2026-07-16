# SyncAcquireResponseDeploymentExposureGenerated

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExposureGenerated } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 312942,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                | *models.SyncAcquireResponseDeploymentCertificateUnion1*                                                      | :heavy_check_mark:                                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                                   |
| `mode`                                                                                                       | [models.SyncAcquireResponseDeploymentModeGenerated](../models/syncacquireresponsedeploymentmodegenerated.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `route`                                                                                                      | *models.SyncAcquireResponseDeploymentRouteUnion1*                                                            | :heavy_check_mark:                                                                                           | Kubernetes route API selected for public endpoints.                                                          |