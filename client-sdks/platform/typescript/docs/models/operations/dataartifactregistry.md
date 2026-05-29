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
      lifecycle: "stopping",
      partial: false,
      stale: false,
    },
    backend: "local",
  },
  resourceType: "artifact-registry",
};
```

## Fields

| Field                    | Type                     | Required                 | Description              |
| ------------------------ | ------------------------ | ------------------------ | ------------------------ |
| `data`                   | *operations.DataUnion12* | :heavy_check_mark:       | N/A                      |
| `resourceType`           | *"artifact-registry"*    | :heavy_check_mark:       | N/A                      |