# DeploymentCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { DeploymentCluster } from "@alienplatform/platform-api/models";

let value: DeploymentCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                          | Type                                                           | Required                                                       | Description                                                    |
| -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- | -------------------------------------------------------------- |
| `cloud`                                                        | *models.DeploymentCloudUnion*                                  | :heavy_minus_sign:                                             | N/A                                                            |
| `namespace`                                                    | *string*                                                       | :heavy_minus_sign:                                             | Namespace where the Alien chart and application resources run. |
| `ownership`                                                    | [models.DeploymentOwnership](../models/deploymentownership.md) | :heavy_check_mark:                                             | Ownership model for the Kubernetes cluster.                    |