# CreateManagerResponseExposureGenerated3

## Example Usage

```typescript
import { CreateManagerResponseExposureGenerated3 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureGenerated3 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `certificate`                                                                                  | *models.CreateManagerResponseCertificateUnion5*                                                | :heavy_check_mark:                                                                             | Certificate publication or reference mode for Kubernetes public endpoints.                     |
| `mode`                                                                                         | [models.CreateManagerResponseModeGenerated3](../models/createmanagerresponsemodegenerated3.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `route`                                                                                        | *models.CreateManagerResponseRouteUnion5*                                                      | :heavy_check_mark:                                                                             | Kubernetes route API selected for public endpoints.                                            |