# SyncAcquireResponseDeploymentGcpClusters

GCP machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentGcpClusters } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentGcpClusters = {
  images: {
    "key": {
      sourceImage: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `images`                                                                                                                             | Record<string, [models.SyncAcquireResponseDeploymentGcpClustersImages](../models/syncacquireresponsedeploymentgcpclustersimages.md)> | :heavy_check_mark:                                                                                                                   | Images by architecture.                                                                                                              |