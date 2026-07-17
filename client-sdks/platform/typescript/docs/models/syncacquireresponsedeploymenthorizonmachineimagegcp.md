# SyncAcquireResponseDeploymentHorizonMachineImageGcp

GCP Horizon machine image catalog.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHorizonMachineImageGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHorizonMachineImageGcp = {
  images: {},
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `images`                                                                                                             | Record<string, [models.SyncAcquireResponseDeploymentGcpImages](../models/syncacquireresponsedeploymentgcpimages.md)> | :heavy_check_mark:                                                                                                   | Images by architecture.                                                                                              |