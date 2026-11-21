# SyncAcquireResponseDeploymentAzureClusters

Azure machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentAzureClusters } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentAzureClusters = {
  images: {},
};
```

## Fields

| Field                                                                                                                                    | Type                                                                                                                                     | Required                                                                                                                                 | Description                                                                                                                              |
| ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                                                 | Record<string, [models.SyncAcquireResponseDeploymentAzureClustersImages](../models/syncacquireresponsedeploymentazureclustersimages.md)> | :heavy_check_mark:                                                                                                                       | Images by architecture.                                                                                                                  |