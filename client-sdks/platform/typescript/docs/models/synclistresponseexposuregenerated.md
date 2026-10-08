# SyncListResponseExposureGenerated

## Example Usage

```typescript
import { SyncListResponseExposureGenerated } from "@alienplatform/platform-api/models";

let value: SyncListResponseExposureGenerated = {
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

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `certificate`                                                                      | *models.SyncListResponseCertificateUnion1*                                         | :heavy_check_mark:                                                                 | Certificate publication or reference mode for Kubernetes public endpoints.         |
| `mode`                                                                             | [models.SyncListResponseModeGenerated](../models/synclistresponsemodegenerated.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `route`                                                                            | *models.SyncListResponseRouteUnion1*                                               | :heavy_check_mark:                                                                 | Kubernetes route API selected for public endpoints.                                |