# ImportSourceExposureGenerated

## Example Usage

```typescript
import { ImportSourceExposureGenerated } from "@alienplatform/platform-api/models";

let value: ImportSourceExposureGenerated = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 728426,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *models.ImportSourceCertificateUnion1*                                     | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [models.ImportSourceModeGenerated](../models/importsourcemodegenerated.md) | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *models.ImportSourceRouteUnion1*                                           | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |