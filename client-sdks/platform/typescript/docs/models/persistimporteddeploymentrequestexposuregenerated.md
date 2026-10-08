# PersistImportedDeploymentRequestExposureGenerated

## Example Usage

```typescript
import { PersistImportedDeploymentRequestExposureGenerated } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                                      | *models.PersistImportedDeploymentRequestCertificateUnion1*                                                         | :heavy_check_mark:                                                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.                                         |
| `mode`                                                                                                             | [models.PersistImportedDeploymentRequestModeGenerated](../models/persistimporteddeploymentrequestmodegenerated.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `route`                                                                                                            | *models.PersistImportedDeploymentRequestRouteUnion1*                                                               | :heavy_check_mark:                                                                                                 | Kubernetes route API selected for public endpoints.                                                                |