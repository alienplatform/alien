# TargetDeploymentHorizonMachineImageGcp

GCP machine image catalog.

## Example Usage

```typescript
import { TargetDeploymentHorizonMachineImageGcp } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizonMachineImageGcp = {
  images: {
    "key": {
      sourceImage: "<value>",
    },
  },
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `images`                                                                                   | Record<string, [models.TargetDeploymentGcpImages](../models/targetdeploymentgcpimages.md)> | :heavy_check_mark:                                                                         | Images by architecture.                                                                    |