# TargetDeploymentAzureClusters

Azure machine image catalog.

## Example Usage

```typescript
import { TargetDeploymentAzureClusters } from "@alienplatform/platform-api/models";

let value: TargetDeploymentAzureClusters = {
  images: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                       | Record<string, [models.TargetDeploymentAzureClustersImages](../models/targetdeploymentazureclustersimages.md)> | :heavy_check_mark:                                                                                             | Images by architecture.                                                                                        |