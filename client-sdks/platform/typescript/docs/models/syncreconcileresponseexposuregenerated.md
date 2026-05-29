# SyncReconcileResponseExposureGenerated

## Example Usage

```typescript
import { SyncReconcileResponseExposureGenerated } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExposureGenerated = {
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

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `certificate`                                                                                | *models.SyncReconcileResponseCertificateUnion1*                                              | :heavy_check_mark:                                                                           | Certificate publication or reference mode for Kubernetes public endpoints.                   |
| `mode`                                                                                       | [models.SyncReconcileResponseModeGenerated](../models/syncreconcileresponsemodegenerated.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `route`                                                                                      | *models.SyncReconcileResponseRouteUnion1*                                                    | :heavy_check_mark:                                                                           | Kubernetes route API selected for public endpoints.                                          |