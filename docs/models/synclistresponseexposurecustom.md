# SyncListResponseExposureCustom

## Example Usage

```typescript
import { SyncListResponseExposureCustom } from "@alienplatform/platform-api/models";

let value: SyncListResponseExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "general-dish.name",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `certificate`                                                                | *models.SyncListResponseCertificateUnion2*                                   | :heavy_check_mark:                                                           | Certificate publication or reference mode for Kubernetes public endpoints.   |
| `domain`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | Hostname routed by the Kubernetes public endpoint.                           |
| `mode`                                                                       | [models.SyncListResponseModeCustom](../models/synclistresponsemodecustom.md) | :heavy_check_mark:                                                           | N/A                                                                          |
| `route`                                                                      | *models.SyncListResponseRouteUnion2*                                         | :heavy_check_mark:                                                           | Kubernetes route API selected for public endpoints.                          |