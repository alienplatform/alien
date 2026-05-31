# KubernetesExposureSettingsGenerated

Use Alien-generated DNS and Platform-managed certificate material.

## Example Usage

```typescript
import { KubernetesExposureSettingsGenerated } from "@alienplatform/manager-api/models";

let value: KubernetesExposureSettingsGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.KubernetesCertificateMode*                                         | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | *"generated"*                                                              | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.KubernetesRouteProfile*                                            | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |