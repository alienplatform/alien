# DataArtifactRegistry

## Example Usage

```typescript
import { DataArtifactRegistry } from "@alienplatform/platform-api/models";

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

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `data`                                                                           | *models.SyncReconcileRequestDataUnion14*                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `resourceType`                                                                   | [models.ResourceTypeArtifactRegistry](../models/resourcetypeartifactregistry.md) | :heavy_check_mark:                                                               | N/A                                                                              |