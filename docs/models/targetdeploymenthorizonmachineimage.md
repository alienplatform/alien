# TargetDeploymentHorizonMachineImage

Horizon machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { TargetDeploymentHorizonMachineImage } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizonMachineImage = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1725531317251",
  gitSha: "<value>",
  horizondArtifacts: {
    "key": {
      sha256: "<value>",
      url: "https://unwieldy-membership.org/",
    },
  },
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                      | *models.TargetDeploymentHorizonMachineImageAwsUnion*                                                       | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `azure`                                                                                                    | *models.HorizonMachineImageConfigAzureUnion*                                                               | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `baseImage`                                                                                                | [models.TargetDeploymentBaseImage](../models/targetdeploymentbaseimage.md)                                 | :heavy_check_mark:                                                                                         | Base image metadata for the Horizon machine image.                                                         |
| `channel`                                                                                                  | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Logical image channel, such as prod, staging, or canary.                                                   |
| `createdAt`                                                                                                | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Image manifest creation timestamp.                                                                         |
| `gcp`                                                                                                      | *models.HorizonMachineImageConfigGcpUnion*                                                                 | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `gitSha`                                                                                                   | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Git commit SHA used to build the image.                                                                    |
| `horizondArtifacts`                                                                                        | Record<string, [models.TargetDeploymentHorizondArtifacts](../models/targetdeploymenthorizondartifacts.md)> | :heavy_check_mark:                                                                                         | Per-architecture horizond artifacts by release-platform key.                                               |
| `horizondVersion`                                                                                          | *string*                                                                                                   | :heavy_check_mark:                                                                                         | horizond daemon version baked into the image.                                                              |
| `machineImageVersion`                                                                                      | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Published immutable machine image version.                                                                 |