# SyncAcquireResponseHorizonMachineImage

Horizon machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonMachineImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonMachineImage = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1717157947859",
  gitSha: "<value>",
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                                            | Type                                                                                                             | Required                                                                                                         | Description                                                                                                      |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `aws`                                                                                                            | *models.SyncAcquireResponseHorizonMachineImageAwsUnion*                                                          | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `azure`                                                                                                          | *models.SyncAcquireResponseHorizonMachineImageAzureUnion*                                                        | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `baseImage`                                                                                                      | [models.SyncAcquireResponseBaseImage](../models/syncacquireresponsebaseimage.md)                                 | :heavy_check_mark:                                                                                               | Base image metadata for the Horizon machine image.                                                               |
| `channel`                                                                                                        | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Logical image channel, such as prod, staging, or canary.                                                         |
| `createdAt`                                                                                                      | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Image manifest creation timestamp.                                                                               |
| `gcp`                                                                                                            | *models.SyncAcquireResponseHorizonMachineImageGcpUnion*                                                          | :heavy_minus_sign:                                                                                               | N/A                                                                                                              |
| `gitSha`                                                                                                         | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Git commit SHA used to build the image.                                                                          |
| `horizondArtifacts`                                                                                              | Record<string, [models.SyncAcquireResponseHorizondArtifacts](../models/syncacquireresponsehorizondartifacts.md)> | :heavy_minus_sign:                                                                                               | Downloadable horizond artifacts keyed by runtime platform, such as `linux-amd64`.                                |
| `horizondVersion`                                                                                                | *string*                                                                                                         | :heavy_check_mark:                                                                                               | horizond daemon version baked into the image.                                                                    |
| `machineImageVersion`                                                                                            | *string*                                                                                                         | :heavy_check_mark:                                                                                               | Published immutable machine image version.                                                                       |