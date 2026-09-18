# DeploymentConfigHorizonMachineImage

Horizon machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { DeploymentConfigHorizonMachineImage } from "@alienplatform/platform-api/models";

let value: DeploymentConfigHorizonMachineImage = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1725419778929",
  gitSha: "<value>",
  horizondArtifacts: {
    "key": {
      sha256: "<value>",
      url: "https://dazzling-pile.org/",
    },
  },
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                                      | Type                                                                                                       | Required                                                                                                   | Description                                                                                                |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                      | *models.DeploymentConfigHorizonMachineImageAwsUnion*                                                       | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `azure`                                                                                                    | *models.DeploymentConfigHorizonMachineImageAzureUnion*                                                     | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `baseImage`                                                                                                | [models.DeploymentConfigBaseImage](../models/deploymentconfigbaseimage.md)                                 | :heavy_check_mark:                                                                                         | Base image metadata for the Horizon machine image.                                                         |
| `channel`                                                                                                  | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Logical image channel, such as prod, staging, or canary.                                                   |
| `createdAt`                                                                                                | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Image manifest creation timestamp.                                                                         |
| `gcp`                                                                                                      | *models.DeploymentConfigHorizonMachineImageGcpUnion*                                                       | :heavy_minus_sign:                                                                                         | N/A                                                                                                        |
| `gitSha`                                                                                                   | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Git commit SHA used to build the image.                                                                    |
| `horizondArtifacts`                                                                                        | Record<string, [models.DeploymentConfigHorizondArtifacts](../models/deploymentconfighorizondartifacts.md)> | :heavy_check_mark:                                                                                         | Per-architecture horizond artifacts by release-platform key.                                               |
| `horizondVersion`                                                                                          | *string*                                                                                                   | :heavy_check_mark:                                                                                         | horizond daemon version baked into the image.                                                              |
| `machineImageVersion`                                                                                      | *string*                                                                                                   | :heavy_check_mark:                                                                                         | Published immutable machine image version.                                                                 |