# KubernetesExposureCustomBaseDomain

## Example Usage

```typescript
import { KubernetesExposureCustomBaseDomain } from "@alienplatform/platform-api/models";

let value: KubernetesExposureCustomBaseDomain = {
  baseDomain: "<value>",
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  mode: "customBaseDomain",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 262621,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `baseDomain`                                                               | *string*                                                                   | :heavy_check_mark:                                                         | DNS name without a wildcard or URL scheme, for example `app.example.com`.  |
| `certificate`                                                              | *models.KubernetesExposureCustomBaseDomainCertificateUnion*                | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [models.ModeCustomBaseDomain](../models/modecustombasedomain.md)           | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.KubernetesExposureCustomBaseDomainRouteUnion*                      | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |