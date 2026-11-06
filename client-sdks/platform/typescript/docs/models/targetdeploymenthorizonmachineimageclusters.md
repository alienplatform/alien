# TargetDeploymentHorizonMachineImageClusters

Machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { TargetDeploymentHorizonMachineImageClusters } from "@alienplatform/platform-api/models";

let value: TargetDeploymentHorizonMachineImageClusters = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1706865303198",
  gitSha: "<value>",
  horizondArtifacts: {
    "key": {
      sha256: "<value>",
      url: "https://radiant-receptor.org/",
    },
  },
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                                                      | Type                                                                                                                       | Required                                                                                                                   | Description                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                                      | *models.TargetDeploymentClustersAwsUnion*                                                                                  | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |
| `azure`                                                                                                                    | *models.ConfigClustersAzureUnion*                                                                                          | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |
| `baseImage`                                                                                                                | [models.TargetDeploymentClustersBaseImage](../models/targetdeploymentclustersbaseimage.md)                                 | :heavy_check_mark:                                                                                                         | Base image metadata for the machine image.                                                                                 |
| `channel`                                                                                                                  | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Logical image channel, such as prod, staging, or canary.                                                                   |
| `createdAt`                                                                                                                | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Image manifest creation timestamp.                                                                                         |
| `gcp`                                                                                                                      | *models.ConfigClustersGcpUnion*                                                                                            | :heavy_minus_sign:                                                                                                         | N/A                                                                                                                        |
| `gitSha`                                                                                                                   | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Git commit SHA used to build the image.                                                                                    |
| `horizondArtifacts`                                                                                                        | Record<string, [models.TargetDeploymentClustersHorizondArtifacts](../models/targetdeploymentclustershorizondartifacts.md)> | :heavy_check_mark:                                                                                                         | Per-architecture machine agent downloads by release-platform key.                                                          |
| `horizondVersion`                                                                                                          | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Machine agent version baked into the image.                                                                                |
| `machineImageVersion`                                                                                                      | *string*                                                                                                                   | :heavy_check_mark:                                                                                                         | Published immutable machine image version.                                                                                 |
| `runtimeIsolationGeneration`                                                                                               | *number*                                                                                                                   | :heavy_minus_sign:                                                                                                         | Runtime isolation capability generation of the immutable artifact, not live readiness.                                     |