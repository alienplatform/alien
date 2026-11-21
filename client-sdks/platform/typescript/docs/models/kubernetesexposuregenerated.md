# KubernetesExposureGenerated

## Example Usage

```typescript
import { KubernetesExposureGenerated } from "@alienplatform/platform-api/models";

let value: KubernetesExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 166428,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.KubernetesExposureGeneratedCertificateUnion*                       | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [models.ModeGenerated](../models/modegenerated.md)                         | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.KubernetesExposureGeneratedRouteUnion*                             | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |