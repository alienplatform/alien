# ImportSourceExposureCustom

## Example Usage

```typescript
import { ImportSourceExposureCustom } from "@alienplatform/platform-api/models";

let value: ImportSourceExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "smart-statue.com",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.ImportSourceCertificateUnion2*                                     | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | [models.ImportSourceModeCustom](../models/importsourcemodecustom.md)       | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.ImportSourceRouteUnion2*                                           | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |