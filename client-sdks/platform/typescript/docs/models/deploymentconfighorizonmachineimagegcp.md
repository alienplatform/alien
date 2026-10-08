# DeploymentConfigHorizonMachineImageGcp

GCP Horizon machine image catalog.

## Example Usage

```typescript
import { DeploymentConfigHorizonMachineImageGcp } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHorizonMachineImageGcp = {
  images: {},
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `images`                                                                                   | Record<string, [models.DeploymentConfigGcpImages](../models/deploymentconfiggcpimages.md)> | :heavy_check_mark:                                                                         | Images by architecture.                                                                    |