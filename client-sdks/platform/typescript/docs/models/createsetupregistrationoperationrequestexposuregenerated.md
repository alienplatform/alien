# CreateSetupRegistrationOperationRequestExposureGenerated

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestExposureGenerated } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                                            | Type                                                                                                                             | Required                                                                                                                         | Description                                                                                                                      |
| -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                                    | *models.CreateSetupRegistrationOperationRequestCertificateUnion1*                                                                | :heavy_check_mark:                                                                                                               | Certificate publication or reference mode for Kubernetes public endpoints.                                                       |
| `mode`                                                                                                                           | [models.CreateSetupRegistrationOperationRequestModeGenerated](../models/createsetupregistrationoperationrequestmodegenerated.md) | :heavy_check_mark:                                                                                                               | N/A                                                                                                                              |
| `route`                                                                                                                          | *models.CreateSetupRegistrationOperationRequestRouteUnion1*                                                                      | :heavy_check_mark:                                                                                                               | Kubernetes route API selected for public endpoints.                                                                              |