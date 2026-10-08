# NewDeploymentRequestExposureCustom

## Example Usage

```typescript
import { NewDeploymentRequestExposureCustom } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestExposureCustom = {
  certificate: {
    mode: "none",
  },
  domain: "hasty-quinoa.biz",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 622555,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `certificate`                                                                        | *models.NewDeploymentRequestCertificateUnion2*                                       | :heavy_check_mark:                                                                   | Certificate publication or reference mode for Kubernetes public endpoints.           |
| `domain`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | Hostname routed by the Kubernetes public endpoint.                                   |
| `mode`                                                                               | [models.NewDeploymentRequestModeCustom](../models/newdeploymentrequestmodecustom.md) | :heavy_check_mark:                                                                   | N/A                                                                                  |
| `route`                                                                              | *models.NewDeploymentRequestRouteUnion2*                                             | :heavy_check_mark:                                                                   | Kubernetes route API selected for public endpoints.                                  |