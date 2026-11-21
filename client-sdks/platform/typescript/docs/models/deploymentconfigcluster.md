# DeploymentConfigCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { DeploymentConfigCluster } from "@alienplatform/platform-api/models";

let value: DeploymentConfigCluster = {
  ownership: "external",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `cloud`                                                                    | *models.DeploymentConfigCloudUnion*                                        | :heavy_minus_sign:                                                         | N/A                                                                        |
| `namespace`                                                                | *string*                                                                   | :heavy_minus_sign:                                                         | Namespace where the Alien chart and application resources run.             |
| `ownership`                                                                | [models.DeploymentConfigOwnership](../models/deploymentconfigownership.md) | :heavy_check_mark:                                                         | Ownership model for the Kubernetes cluster.                                |