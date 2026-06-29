# PlanDeploymentComputeCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { PlanDeploymentComputeCluster } from "@alienplatform/platform-api/models/operations";

let value: PlanDeploymentComputeCluster = {
  ownership: "external",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `cloud`                                                                                                | *operations.PlanDeploymentComputeCloudUnion*                                                           | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `namespace`                                                                                            | *string*                                                                                               | :heavy_minus_sign:                                                                                     | Namespace where the Alien chart and application resources run.                                         |
| `ownership`                                                                                            | [operations.PlanDeploymentComputeOwnership](../../models/operations/plandeploymentcomputeownership.md) | :heavy_check_mark:                                                                                     | Ownership model for the Kubernetes cluster.                                                            |