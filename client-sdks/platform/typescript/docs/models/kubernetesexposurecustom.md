# KubernetesExposureCustom

## Example Usage

```typescript
import { KubernetesExposureCustom } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "fearless-hammock.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | *models.KubernetesExposureCustomCertificateUnion*                                            | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `domain`                                                                                     | *string*                                                                                     | :heavy_check_mark:                                                                           | Hostname routed by the Kubernetes public endpoint.                                           |
| `mode`                                                                                       | [models.KubernetesExposureCustomModeCustom](../models/kubernetesexposurecustommodecustom.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.KubernetesExposureCustomRouteUnion*                                                  | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |