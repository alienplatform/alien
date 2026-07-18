# SyncAcquireResponseDeploymentHorizonMachineImage

Horizon machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentHorizonMachineImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentHorizonMachineImage = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1708145887515",
  gitSha: "<value>",
  horizondArtifacts: {},
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                                                                | Type                                                                                                                                 | Required                                                                                                                             | Description                                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------ |
| `aws`                                                                                                                                | *models.SyncAcquireResponseDeploymentHorizonMachineImageAwsUnion*                                                                    | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `azure`                                                                                                                              | *models.SyncAcquireResponseDeploymentHorizonMachineImageAzureUnion*                                                                  | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `baseImage`                                                                                                                          | [models.SyncAcquireResponseDeploymentBaseImage](../models/syncacquireresponsedeploymentbaseimage.md)                                 | :heavy_check_mark:                                                                                                                   | Base image metadata for the Horizon machine image.                                                                                   |
| `channel`                                                                                                                            | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Logical image channel, such as prod, staging, or canary.                                                                             |
| `createdAt`                                                                                                                          | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Image manifest creation timestamp.                                                                                                   |
| `gcp`                                                                                                                                | *models.SyncAcquireResponseDeploymentHorizonMachineImageGcpUnion*                                                                    | :heavy_minus_sign:                                                                                                                   | N/A                                                                                                                                  |
| `gitSha`                                                                                                                             | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Git commit SHA used to build the image.                                                                                              |
| `horizondArtifacts`                                                                                                                  | Record<string, [models.SyncAcquireResponseDeploymentHorizondArtifacts](../models/syncacquireresponsedeploymenthorizondartifacts.md)> | :heavy_check_mark:                                                                                                                   | Per-architecture horizond artifacts by release-platform key.                                                                         |
| `horizondVersion`                                                                                                                    | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | horizond daemon version baked into the image.                                                                                        |
| `machineImageVersion`                                                                                                                | *string*                                                                                                                             | :heavy_check_mark:                                                                                                                   | Published immutable machine image version.                                                                                           |