# SyncAcquireResponseExternalBindingsGar

Google Artifact Registry binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsGar } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseExternalBindingsGar = {
  service: "gar",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `pullServiceAccountEmail`                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `pushServiceAccountEmail`                                                                                            | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"gar"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeArtifactRegistry3](../models/syncacquireresponsetypeartifactregistry3.md)             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |