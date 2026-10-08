# NewDeploymentRequestCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { NewDeploymentRequestCluster } from "@alienplatform/platform-api/models";

let value: NewDeploymentRequestCluster = {
  ownership: "external",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `cloud`                                                                            | *models.NewDeploymentRequestCloudUnion*                                            | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `namespace`                                                                        | *string*                                                                           | :heavy_minus_sign:                                                                 | Namespace where the Alien chart and application resources run.                     |
| `ownership`                                                                        | [models.NewDeploymentRequestOwnership](../models/newdeploymentrequestownership.md) | :heavy_check_mark:                                                                 | Ownership model for the Kubernetes cluster.                                        |