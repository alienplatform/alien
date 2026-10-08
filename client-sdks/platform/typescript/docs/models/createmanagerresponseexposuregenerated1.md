# CreateManagerResponseExposureGenerated1

## Example Usage

```typescript
import { CreateManagerResponseExposureGenerated1 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureGenerated1 = {
  certificate: {
    mode: "managedAcmImport",
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
| `certificate`                                                                                  | *models.CreateManagerResponseCertificateUnion1*                                                | :heavy_check_mark:                                                                             | Certificate publication or reference mode for Kubernetes public endpoints.                     |
| `mode`                                                                                         | [models.CreateManagerResponseModeGenerated1](../models/createmanagerresponsemodegenerated1.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `route`                                                                                        | *models.CreateManagerResponseRouteUnion1*                                                      | :heavy_check_mark:                                                                             | Kubernetes route API selected for public endpoints.                                            |