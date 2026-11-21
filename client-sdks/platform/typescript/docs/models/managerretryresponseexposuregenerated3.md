# ManagerRetryResponseExposureGenerated3

## Example Usage

```typescript
import { ManagerRetryResponseExposureGenerated3 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureGenerated3 = {
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

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | *models.ManagerRetryResponseCertificateUnion5*                                               | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `mode`                                                                                       | [models.ManagerRetryResponseModeGenerated3](../models/managerretryresponsemodegenerated3.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.ManagerRetryResponseRouteUnion5*                                                     | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |