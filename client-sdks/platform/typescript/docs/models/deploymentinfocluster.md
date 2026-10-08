# DeploymentInfoCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { DeploymentInfoCluster } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `cloud`                                                                | *models.DeploymentInfoCloudUnion*                                      | :heavy_minus_sign:                                                     | N/A                                                                    |
| `namespace`                                                            | *string*                                                               | :heavy_minus_sign:                                                     | Namespace where the Alien chart and application resources run.         |
| `ownership`                                                            | [models.DeploymentInfoOwnership](../models/deploymentinfoownership.md) | :heavy_check_mark:                                                     | Ownership model for the Kubernetes cluster.                            |