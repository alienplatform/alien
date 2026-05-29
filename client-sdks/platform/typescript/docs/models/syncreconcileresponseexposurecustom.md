# SyncReconcileResponseExposureCustom

## Example Usage

```typescript
import { SyncReconcileResponseExposureCustom } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "difficult-switchboard.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `certificate`                                                                          | *models.SyncReconcileResponseCertificateUnion2*                                        | :heavy_check_mark:                                                                     | Certificate publication or reference mode for Kubernetes public endpoints.             |
| `domain`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Hostname routed by the Kubernetes public endpoint.                                     |
| `mode`                                                                                 | [models.SyncReconcileResponseModeCustom](../models/syncreconcileresponsemodecustom.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |
| `route`                                                                                | *models.SyncReconcileResponseRouteUnion2*                                              | :heavy_check_mark:                                                                     | Kubernetes route API selected for public endpoints.                                    |