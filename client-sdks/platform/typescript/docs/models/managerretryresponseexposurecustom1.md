# ManagerRetryResponseExposureCustom1

## Example Usage

```typescript
import { ManagerRetryResponseExposureCustom1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureCustom1 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "ironclad-appliance.info",
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
| `certificate`                                                                          | *models.ManagerRetryResponseCertificateUnion2*                                         | :heavy_check_mark:                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.             |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Hostname routed by the Kubernetes public endpoint.                                     |
| `mode`                                                                                 | [models.ManagerRetryResponseModeCustom1](../models/managerretryresponsemodecustom1.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `route`                                                                                | *models.ManagerRetryResponseRouteUnion2*                                               | :heavy_check_mark:                                                                     | Kubernetes route API selected for public endpoints.                                    |