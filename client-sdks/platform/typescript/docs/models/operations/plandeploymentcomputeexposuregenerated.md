# PlanDeploymentComputeExposureGenerated

## Example Usage

```typescript
import { PlanDeploymentComputeExposureGenerated } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeExposureGenerated = {
  certificate: {
    mode: "managedTlsSecret",
    secretNameTemplate: "<value>",
  },
  mode: "generated",
  route: {
    gatewayClassName: "<value>",
    listenerPort: 163968,
    routeApi: "gateway",
  },
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `certificate`                                                                                                  | *operations.PlanDeploymentComputeCertificateUnion1*                                                            | :heavy_check_mark:                                                                                             | Certificate publication or reference mode for Kubernetes public endpoints.                                     |
| `mode`                                                                                                         | [operations.PlanDeploymentComputeModeGenerated](../../models/operations/plandeploymentcomputemodegenerated.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |
| `route`                                                                                                        | *operations.PlanDeploymentComputeRouteUnion1*                                                                  | :heavy_check_mark:                                                                                             | Kubernetes route API selected for public endpoints.                                                            |