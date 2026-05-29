# ExposureGenerated

## Example Usage

```typescript
import { ExposureGenerated } from "@alienplatform/platform-api/models/operations";

let value: ExposureGenerated = {
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

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `certificate`                                                              | *operations.CertificateUnion1*                                             | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `mode`                                                                     | [operations.ModeGenerated](../../models/operations/modegenerated.md)       | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *operations.RouteUnion1*                                                   | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |