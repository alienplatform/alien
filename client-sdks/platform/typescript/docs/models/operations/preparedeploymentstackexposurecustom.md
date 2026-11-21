# PrepareDeploymentStackExposureCustom

## Example Usage

```typescript
import { PrepareDeploymentStackExposureCustom } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackExposureCustom = {
  certificate: {
    certificateArn: "<value>",
    mode: "awsAcmArn",
  },
  domain: "messy-chainstay.org",
  mode: "custom",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 185232,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                              | *operations.PrepareDeploymentStackCertificateUnion2*                                                       | :heavy_check_mark:                                                                                         | Certificate publication or reference mode for Kubernetes public endpoints.                                 |
| `domain`                                                                                                   | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Hostname routed by the Kubernetes public endpoint.                                                         |
| `mode`                                                                                                     | [operations.PrepareDeploymentStackModeCustom](../../models/operations/preparedeploymentstackmodecustom.md) | :heavy_check_mark:                                                                                         | N/A                                                                                                        |
| `route`                                                                                                    | *operations.PrepareDeploymentStackRouteUnion2*                                                             | :heavy_check_mark:                                                                                         | Kubernetes route API selected for public endpoints.                                                        |