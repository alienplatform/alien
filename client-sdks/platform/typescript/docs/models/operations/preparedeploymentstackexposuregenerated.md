# PrepareDeploymentStackExposureGenerated

## Example Usage

```typescript
import { PrepareDeploymentStackExposureGenerated } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackExposureGenerated = {
  certificate: {
    mode: "none",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 60732,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                    | *operations.PrepareDeploymentStackCertificateUnion1*                                                             | :heavy_check_mark:                                                                                               | Certificate publication or reference mode for Kubernetes public endpoints.                                       |
| `mode`                                                                                                           | [operations.PrepareDeploymentStackModeGenerated](../../models/operations/preparedeploymentstackmodegenerated.md) | :heavy_check_mark:                                                                                               | N/A                                                                                                              |
| `route`                                                                                                          | *operations.PrepareDeploymentStackRouteUnion1*                                                                   | :heavy_check_mark:                                                                                               | Kubernetes route API selected for public endpoints.                                                              |