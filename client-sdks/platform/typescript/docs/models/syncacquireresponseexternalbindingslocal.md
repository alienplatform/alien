# SyncAcquireResponseExternalBindingsLocal

Local container registry binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsLocal } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsLocal = {
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
| `type`                                                                                                               | [models.SyncAcquireResponseTypeArtifactRegistry4](../models/syncacquireresponsetypeartifactregistry4.md)             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |