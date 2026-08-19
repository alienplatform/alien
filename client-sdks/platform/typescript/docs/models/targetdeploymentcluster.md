# TargetDeploymentCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { TargetDeploymentCluster } from "@alienplatform/platform-api/models";

let value: TargetDeploymentCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cloud`                                                                    | *models.TargetDeploymentCloudUnion*                                        | :heavy_minus_sign:                                                         | N/A                                                                        |
| `namespace`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | Namespace where the Alien chart and application resources run.             |
| `ownership`                                                                | [models.TargetDeploymentOwnership](../models/targetdeploymentownership.md) | :heavy_check_mark:                                                         | Ownership model for the Kubernetes cluster.                                |