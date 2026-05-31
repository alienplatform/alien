# DeploymentDetailResponseCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { DeploymentDetailResponseCluster } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseCluster = {
  ownership: "managed",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `cloud`                                                                                    | *models.DeploymentDetailResponseCloudUnion*                                                | :heavy_minus_sign:                                                                         | N/A                                                                                        |
| `namespace`                                                                                | *string*                                                                                   | :heavy_minus_sign:                                                                         | Namespace where the Alien chart and application resources run.                             |
| `ownership`                                                                                | [models.DeploymentDetailResponseOwnership](../models/deploymentdetailresponseownership.md) | :heavy_check_mark:                                                                         | Ownership model for the Kubernetes cluster.                                                |