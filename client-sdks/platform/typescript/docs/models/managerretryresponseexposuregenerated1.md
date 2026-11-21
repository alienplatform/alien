# ManagerRetryResponseExposureGenerated1

## Example Usage

```typescript
import { ManagerRetryResponseExposureGenerated1 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureGenerated1 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | *models.ManagerRetryResponseCertificateUnion1*                                               | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `mode`                                                                                       | [models.ManagerRetryResponseModeGenerated1](../models/managerretryresponsemodegenerated1.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.ManagerRetryResponseRouteUnion1*                                                     | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |