# CloudFormationCallbackRequestExposureCustom

## Example Usage

```typescript
import { CloudFormationCallbackRequestExposureCustom } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "courteous-descent.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `certificate`                                                                                          | *models.CloudFormationCallbackRequestCertificateUnion2*                                                | :heavy_check_mark:                                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.                             |
| `domain`                                                                                               | *string*                                                                                               | :heavy_check_mark:                                                                                     | Hostname routed by the Kubernetes public endpoint.                                                     |
| `mode`                                                                                                 | [models.CloudFormationCallbackRequestModeCustom](../models/cloudformationcallbackrequestmodecustom.md) | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `route`                                                                                                | *models.CloudFormationCallbackRequestRouteUnion2*                                                      | :heavy_check_mark:                                                                                     | Kubernetes route API selected for public endpoints.                                                    |