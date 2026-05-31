# SyncAcquireResponseExposureCustom

## Example Usage

```typescript
import { SyncAcquireResponseExposureCustom } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExposureCustom = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  domain: "flickering-rule.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `certificate`                                                                      | *models.SyncAcquireResponseCertificateUnion2*                                      | :heavy_check_mark:                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.         |
| `domain`                                                                           | *string*                                                                           | :heavy_check_mark:                                                                 | Hostname routed by the Kubernetes public endpoint.                                 |
| `mode`                                                                             | [models.SyncAcquireResponseModeCustom](../models/syncacquireresponsemodecustom.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `route`                                                                            | *models.SyncAcquireResponseRouteUnion2*                                            | :heavy_check_mark:                                                                 | Kubernetes route API selected for public endpoints.                                |