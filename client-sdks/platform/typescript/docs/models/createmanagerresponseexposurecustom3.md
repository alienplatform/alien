# CreateManagerResponseExposureCustom3

## Example Usage

```typescript
import { CreateManagerResponseExposureCustom3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureCustom3 = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "spirited-alb.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | *models.CreateManagerResponseCertificateUnion6*                                          | :heavy_check_mark:                                                                       | Certificate publication or reference mode for Kubernetes public endpoints.               |
| `domain`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Hostname routed by the Kubernetes public endpoint.                                       |
| `mode`                                                                                   | [models.CreateManagerResponseModeCustom3](../models/createmanagerresponsemodecustom3.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `route`                                                                                  | *models.CreateManagerResponseRouteUnion6*                                                | :heavy_check_mark:                                                                       | Kubernetes route API selected for public endpoints.                                      |