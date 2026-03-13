# SyncAcquireResponseExternalBindingsAcr

Azure Container Registry binding configuration

## Example Usage

```typescript
import { SyncAcquireResponseExternalBindingsAcr } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseExternalBindingsAcr = {
  service: "acr",
  type: "artifact_registry",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `registryName`                                                                                                       | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `resourceGroupName`                                                                                                  | *any*                                                                                                                | :heavy_minus_sign:                                                                                                   | Represents a value that can be either a concrete value, a template expression,<br/>or a reference to a Kubernetes Secret |
| `service`                                                                                                            | *"acr"*                                                                                                              | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |
| `type`                                                                                                               | [models.SyncAcquireResponseTypeArtifactRegistry2](../models/syncacquireresponsetypeartifactregistry2.md)             | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |