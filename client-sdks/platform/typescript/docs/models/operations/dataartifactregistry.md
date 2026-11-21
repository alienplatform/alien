# DataArtifactRegistry

## Example Usage

```typescript
import { DataArtifactRegistry } from "@alienplatform/platform-api/models/operations";

let value: DataArtifactRegistry = {
  data: {
    reachable: false,
    registryUrl: "https://tedious-reach.com",
    status: {
      collectionIssues: [],
      health: "unknown",
      lifecycle: "creating",
      partial: false,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "artifact-registry",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `data`                                                                                             | *operations.DataUnion14*                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `resourceType`                                                                                     | [operations.ResourceTypeArtifactRegistry](../../models/operations/resourcetypeartifactregistry.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |