# CreateSetupRegistrationOperationRequestExposureCustom

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestExposureCustom } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "snappy-petal.info",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                              | *models.CreateSetupRegistrationOperationRequestCertificateUnion2*                                                          | :heavy_check_mark:                                                                                                         | Certificate publication or reference mode for Kubernetes public endpoints.                                                 |
| `domain`                                                                                                                   | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Hostname routed by the Kubernetes public endpoint.                                                                         |
| `mode`                                                                                                                     | [models.CreateSetupRegistrationOperationRequestModeCustom](../models/createsetupregistrationoperationrequestmodecustom.md) | :heavy_check_mark:                                                                                                         | N/A                                                                                                                        |
| `route`                                                                                                                    | *models.CreateSetupRegistrationOperationRequestRouteUnion2*                                                                | :heavy_check_mark:                                                                                                         | Kubernetes route API selected for public endpoints.                                                                        |