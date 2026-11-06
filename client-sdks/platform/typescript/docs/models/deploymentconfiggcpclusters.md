# DeploymentConfigGcpClusters

GCP machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigGcpClusters } from "@alienplatform/platform-api/models";

let value: DeploymentConfigGcpClusters = {
  images: {},
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                   | Record<string, [models.DeploymentConfigGcpClustersImages](../models/deploymentconfiggcpclustersimages.md)> | :heavy_check_mark:                                                                                         | Images by architecture.                                                                                    |