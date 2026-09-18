# ManagerRetryResponseExposureGenerated2

## Example Usage

```typescript
import { ManagerRetryResponseExposureGenerated2 } from "@alienplatform/platform-api/models";

let value: ManagerRetryResponseExposureGenerated2 = {
  certificate: {
    mode: "managedAcmImport",
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
| `certificate`                                                                                | *models.ManagerRetryResponseCertificateUnion3*                                               | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `mode`                                                                                       | [models.ManagerRetryResponseModeGenerated2](../models/managerretryresponsemodegenerated2.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.ManagerRetryResponseRouteUnion3*                                                     | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |