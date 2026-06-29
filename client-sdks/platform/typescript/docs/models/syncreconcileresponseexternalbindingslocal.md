# SyncReconcileResponseExternalBindingsLocal

Local container registry binding configuration.

The local registry runs on localhost only and does not require authentication.
Security boundary is the OS process isolation on the customer's machine.
External image access is secured by the manager's registry proxy (deployment tokens).

## Example Usage

```typescript
import { SyncReconcileResponseExternalBindingsLocal } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseExternalBindingsLocal = {
  service: "local",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `dataDir`                                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `registryUrl`                                                                                                        | *models.SyncReconcileResponseRegistryUrlUnion*                                                                       | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.TargetTypeArtifactRegistry4](../models/targettypeartifactregistry4.md)                                       | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |