# PersistImportedDeploymentRequestExposureCustom

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExposureCustom } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "male-freckle.name",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 143133,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                | *models.PersistImportedDeploymentRequestCertificateUnion2*                                                   | :heavy_check_mark:                                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                                   |
| `domain`                                                                                                     | *string*                                                                                                     | :heavy_check_mark:                                                                                           | Hostname routed by the Kubernetes public endpoint.                                                           |
| `mode`                                                                                                       | [models.PersistImportedDeploymentRequestModeCustom](../models/persistimporteddeploymentrequestmodecustom.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `route`                                                                                                      | *models.PersistImportedDeploymentRequestRouteUnion2*                                                         | :heavy_check_mark:                                                                                           | Kubernetes route API selected for public endpoints.                                                          |