# ManagerRetryResponseExposureCustom3

## Example Usage

```typescript
import { ManagerRetryResponseExposureCustom3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureCustom3 = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "unconscious-begonia.name",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `certificate`                                                                          | *models.ManagerRetryResponseCertificateUnion6*                                         | :heavy_check_mark:                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.             |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Hostname routed by the Kubernetes public endpoint.                                     |
| `mode`                                                                                 | [models.ManagerRetryResponseModeCustom3](../models/managerretryresponsemodecustom3.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `route`                                                                                | *models.ManagerRetryResponseRouteUnion6*                                               | :heavy_check_mark:                                                                     | Kubernetes route API selected for public endpoints.                                    |