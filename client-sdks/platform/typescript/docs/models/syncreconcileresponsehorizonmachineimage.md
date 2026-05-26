# SyncReconcileResponseHorizonMachineImage

Horizon machine image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { SyncReconcileResponseHorizonMachineImage } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseHorizonMachineImage = {
  baseImage: {
    name: "<value>",
    version: "<value>",
  },
  channel: "<value>",
  createdAt: "1726202235588",
  gitSha: "<value>",
  horizondVersion: "<value>",
  machineImageVersion: "<value>",
};
```

## Fields

| Field                                                                                | Type                                                                                 | Required                                                                             | Description                                                                          |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `aws`                                                                                | *models.SyncReconcileResponseHorizonMachineImageAwsUnion*                            | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `azure`                                                                              | *models.HorizonMachineImageTargetAzureUnion*                                         | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `baseImage`                                                                          | [models.SyncReconcileResponseBaseImage](../models/syncreconcileresponsebaseimage.md) | :heavy_check_mark:                                                                   | Base image metadata for the Horizon machine image.                                   |
| `channel`                                                                            | *string*                                                                             | :heavy_check_mark:                                                                   | Logical image channel, such as prod, staging, or canary.                             |
| `createdAt`                                                                          | *string*                                                                             | :heavy_check_mark:                                                                   | Image manifest creation timestamp.                                                   |
| `gcp`                                                                                | *models.HorizonMachineImageTargetGcpUnion*                                           | :heavy_minus_sign:                                                                   | N/A                                                                                  |
| `gitSha`                                                                             | *string*                                                                             | :heavy_check_mark:                                                                   | Git commit SHA used to build the image.                                              |
| `horizondVersion`                                                                    | *string*                                                                             | :heavy_check_mark:                                                                   | horizond daemon version baked into the image.                                        |
| `machineImageVersion`                                                                | *string*                                                                             | :heavy_check_mark:                                                                   | Published immutable machine image version.                                           |