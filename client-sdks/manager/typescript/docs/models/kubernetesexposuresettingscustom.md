# KubernetesExposureSettingsCustom

Use a customer hostname and customer-owned certificate reference.

## Example Usage

```typescript
import { KubernetesExposureSettingsCustom } from "@alienplatform/manager-api/models";

let value: KubernetesExposureSettingsCustom = {
  certificate: {
    mode: "none",
  },
  domain: "quick-witted-wallaby.org",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 466794,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.KubernetesCertificateMode*                                         | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | *"custom"*                                                                 | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.KubernetesRouteProfile*                                            | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |