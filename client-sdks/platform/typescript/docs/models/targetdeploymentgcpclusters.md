# TargetDeploymentGcpClusters

GCP machine image catalog.

## Example Usage

```typescript
import { TargetDeploymentGcpClusters } from "@alienplatform/platform-api/models";

let value: TargetDeploymentGcpClusters = {
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