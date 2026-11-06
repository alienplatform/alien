# DeploymentConfigAzureClusters

Azure machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigAzureClusters } from "@alienplatform/platform-api/models";

let value: DeploymentConfigAzureClusters = {
  images: {},
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                       | Record<string, [models.DeploymentConfigAzureClustersImages](../models/deploymentconfigazureclustersimages.md)> | :heavy_check_mark:                                                                                             | Images by architecture.                                                                                        |