# KubernetesExposureSettingsCustomBaseDomain

Give each named public endpoint a hostname below a customer-owned base domain.

## Example Usage

```typescript
import { KubernetesExposureSettingsCustomBaseDomain } from "@alienplatform/manager-api/models";

let value: KubernetesExposureSettingsCustomBaseDomain = {
  baseDomain: "<value>",
  certificate: {
    mode: "none",
  },
  mode: "customBaseDomain",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 745747,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `baseDomain`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | DNS name without a wildcard or URL scheme, for example `app.example.com`.  |
| `certificate`                                                              | *models.KubernetesCertificateMode*                                         | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | *"customBaseDomain"*                                                       | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.KubernetesRouteProfile*                                            | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |
