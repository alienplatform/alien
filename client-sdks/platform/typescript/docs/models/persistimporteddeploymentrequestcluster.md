# PersistImportedDeploymentRequestCluster

Kubernetes cluster setup settings.

## Example Usage

```typescript
import { PersistImportedDeploymentRequestCluster } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestCluster = {
  ownership: "existing",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `cloud`                                                                                                    | *models.PersistImportedDeploymentRequestCloudUnion*                                                        | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `namespace`                                                                                                | *string*                                                                                                   | :heavy_minus_sign:                                                                                         | Namespace where the Alien chart and application resources run.                                             |
| `ownership`                                                                                                | [models.PersistImportedDeploymentRequestOwnership](../models/persistimporteddeploymentrequestownership.md) | :heavy_check_mark:                                                                                         | Ownership model for the Kubernetes cluster.                                                                |