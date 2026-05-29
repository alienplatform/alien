# DataArtifactRegistry

## Example Usage

```typescript
import { DataArtifactRegistry } from "@alienplatform/platform-api/models/operations";

let value: DataArtifactRegistry = {
  data: {
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2026-05-25T20:34:34.708Z"),
        severity: "error",
      },
    ],
    reachable: false,
    registryUrl: "https://international-consistency.net/",
    status: {
      collectionIssues: [],
      health: "degraded",
      lifecycle: "failed",
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