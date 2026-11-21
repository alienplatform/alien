# ManagerRetryResponseExposureCustom2

## Example Usage

```typescript
import { ManagerRetryResponseExposureCustom2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureCustom2 = {
  certificate: {
    mode: "none",
  },
  domain: "immediate-phrase.info",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 665226,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `certificate`                                                                          | *models.ManagerRetryResponseCertificateUnion4*                                         | :heavy_check_mark:                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.             |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Hostname routed by the Kubernetes public endpoint.                                     |
| `mode`                                                                                 | [models.ManagerRetryResponseModeCustom2](../models/managerretryresponsemodecustom2.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `route`                                                                                | *models.ManagerRetryResponseRouteUnion4*                                               | :heavy_check_mark:                                                                     | Kubernetes route API selected for public endpoints.                                    |