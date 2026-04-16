# SyncReconcileResponseExternalBindingsAcr

Azure Container Registry binding configuration

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsAcr } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsAcr = {
  service: "acr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `registryName`                                                                                                       | *models.SyncReconcileResponseRegistryNameUnion*                                                                      | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *models.SyncReconcileResponseResourceGroupNameUnion2*                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"acr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeArtifactRegistry2](../models/syncreconcileresponsetypeartifactregistry2.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |