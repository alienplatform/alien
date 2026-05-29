# ExposureCustom

## Example Usage

```typescript
import { ExposureCustom } from "@alienplatform/platform-api/models/operations";

let value: ExposureCustom = {
  certificate: {
    mode: "managedAcmImport",
  },
  domain: "miserable-planula.info",
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
| `certificate`                                                              | *operations.CertificateUnion2*                                             | :heavy_check_mark:                                                         | Certificate publication or reference mode for Kubernetes public endpoints. |
| `domain`                                                                   | *string*                                                                   | :heavy_check_mark:                                                         | Hostname routed by the Kubernetes public endpoint.                         |
| `mode`                                                                     | [operations.ModeCustom](../../models/operations/modecustom.md)             | :heavy_check_mark:                                                         | N/A                                                                        |
| `route`                                                                    | *operations.RouteUnion2*                                                   | :heavy_check_mark:                                                         | Kubernetes route API selected for public endpoints.                        |