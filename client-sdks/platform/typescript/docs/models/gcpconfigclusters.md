# GcpConfigClusters

GCP machine image catalog.

## Example Usage

```typescript
import { GcpConfigClusters } from "@alienplatform/platform-api/models";

let value: GcpConfigClusters = {
  images: {
    "key": {
      sourceImage: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                   | Record<string, [models.TargetDeploymentGcpClustersImages](../models/targetdeploymentgcpclustersimages.md)> | :heavy_check_mark:                                                                                         | Images by architecture.                                                                                    |