# CreateManagerResponseExposureGenerated2

## Example Usage

```typescript
import { CreateManagerResponseExposureGenerated2 } from "@alienplatform/platform-api/models";

let value: CreateManagerResponseExposureGenerated2 = {
  certificate: {
    secretName: "<value>",
    mode: "tlsSecretRef",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 938185,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `certificate`                                                                                  | *models.CreateManagerResponseCertificateUnion3*                                                | :heavy_check_mark:                                                                             | Certificate publication or reference mode for Kubernetes public endpoints.                     |
| `mode`                                                                                         | [models.CreateManagerResponseModeGenerated2](../models/createmanagerresponsemodegenerated2.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
| `route`                                                                                        | *models.CreateManagerResponseRouteUnion3*                                                      | :heavy_check_mark:                                                                             | Kubernetes route API selected for public endpoints.                                            |