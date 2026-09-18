# PrepareDeploymentStackCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { PrepareDeploymentStackCluster } from "@alienplatform/platform-api/models/operations";

let value: PrepareDeploymentStackCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `cloud`                                                                                                  | *operations.PrepareDeploymentStackCloudUnion*                                                            | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `namespace`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | Namespace where the Alien chart and application resources run.                                           |
| `ownership`                                                                                              | [operations.PrepareDeploymentStackOwnership](../../models/operations/preparedeploymentstackownership.md) | :heavy_check_mark:                                                                                       | Ownership model for the Kubernetes cluster.                                                              |