# SyncReconcileResponseExternalBindingsLocal

Local container registry binding configuration

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
| `registryUrl`                                                                                                        | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"local"*                                                                                                            | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncReconcileResponseTypeArtifactRegistry4](../models/syncreconcileresponsetypeartifactregistry4.md)         | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |