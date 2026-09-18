# HorizonMachineImageGcpConfig

GCP Horizon machine image catalog.

## Example Usage

```typescript
import { HorizonMachineImageGcpConfig } from "@alienplatform/platform-api/models";

let value: HorizonMachineImageGcpConfig = {
  images: {},
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `images`                                                                                   | Record<string, [models.TargetDeploymentGcpImages](../models/targetdeploymentgcpimages.md)> | :heavy_check_mark:                                                                         | Images by architecture.                                                                    |