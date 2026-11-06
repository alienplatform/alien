# AzureConfigClusters

Azure machine image catalog.

## Example Usage

```typescript
import { AzureConfigClusters } from "@alienplatform/platform-api/models";

let value: AzureConfigClusters = {
  images: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                       | Record<string, [models.TargetDeploymentAzureClustersImages](../models/targetdeploymentazureclustersimages.md)> | :heavy_check_mark:                                                                                             | Images by architecture.                                                                                        |