# PlanDeploymentComputeExposureCustom

## Example Usage

```typescript
import { PlanDeploymentComputeExposureCustom } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeExposureCustom = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  domain: "unsightly-guard.info",
  mode: "custom",
  route: {
    ingressClassName: "<value>",
    routeApi: "ingress",
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                            | *operations.PlanDeploymentComputeCertificateUnion2*                                                      | :heavy_check_mark:                                                                                       | Certificate publication or reference mode for Kubernetes public endpoints.                               |
| `domain`                                                                                                 | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Hostname routed by the Kubernetes public endpoint.                                                       |
| `mode`                                                                                                   | [operations.PlanDeploymentComputeModeCustom](../../models/operations/plandeploymentcomputemodecustom.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `route`                                                                                                  | *operations.PlanDeploymentComputeRouteUnion2*                                                            | :heavy_check_mark:                                                                                       | Kubernetes route API selected for public endpoints.                                                      |