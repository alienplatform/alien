# SyncAcquireResponseExposureGenerated

## Example Usage

```typescript
import { SyncAcquireResponseExposureGenerated } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExposureGenerated = {
  certificate: {
    mode: "managedAcmImport",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 168781,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | *models.SyncAcquireResponseCertificateUnion1*                                            | :heavy_check_mark:                                                                       | Certificate publication or reference mode for Kubernetes public endpoints.               |
| `mode`                                                                                   | [models.SyncAcquireResponseModeGenerated](../models/syncacquireresponsemodegenerated.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `route`                                                                                  | *models.SyncAcquireResponseRouteUnion1*                                                  | :heavy_check_mark:                                                                       | Kubernetes route API selected for public endpoints.                                      |