# CreateManagerResponseExposureCustom2

## Example Usage

```typescript
import { CreateManagerResponseExposureCustom2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureCustom2 = {
  certificate: {
    mode: "none",
  },
  domain: "baggy-verve.biz",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 581113,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | *models.CreateManagerResponseCertificateUnion4*                                          | :heavy_check_mark:                                                                       | Certificate publication or reference mode for Kubernetes public endpoints.               |
| `domain`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Hostname routed by the Kubernetes public endpoint.                                       |
| `mode`                                                                                   | [models.CreateManagerResponseModeCustom2](../models/createmanagerresponsemodecustom2.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `route`                                                                                  | *models.CreateManagerResponseRouteUnion4*                                                | :heavy_check_mark:                                                                       | Kubernetes route API selected for public endpoints.                                      |