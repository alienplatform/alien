# SyncAcquireResponseDeploymentExposureCustom

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentExposureCustom } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "royal-innovation.com",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 829658,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                          | *models.SyncAcquireResponseDeploymentCertificateUnion2*                                                | :heavy_check_mark:                                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.                             |
| `domain`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Hostname routed by the Kubernetes public endpoint.                                                     |
| `mode`                                                                                                 | [models.SyncAcquireResponseDeploymentModeCustom](../models/syncacquireresponsedeploymentmodecustom.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `route`                                                                                                | *models.SyncAcquireResponseDeploymentRouteUnion2*                                                      | :heavy_check_mark:                                                                                     | Kubernetes route API selected for public endpoints.                                                    |