# CreateManagerResponseExposureCustom1

## Example Usage

```typescript
import { CreateManagerResponseExposureCustom1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureCustom1 = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "fatal-hutch.org",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `certificate`                                                                            | *models.CreateManagerResponseCertificateUnion2*                                          | :heavy_check_mark:                                                                       | Certificate publication or reference mode for Kubernetes public endpoints.               |
| `domain`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | Hostname routed by the Kubernetes public endpoint.                                       |
| `mode`                                                                                   | [models.CreateManagerResponseModeCustom1](../models/createmanagerresponsemodecustom1.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |
| `route`                                                                                  | *models.CreateManagerResponseRouteUnion2*                                                | :heavy_check_mark:                                                                       | Kubernetes route API selected for public endpoints.                                      |