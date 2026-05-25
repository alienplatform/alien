# SyncAcquireResponseHorizonHostImage

Horizon host image catalog.

Platform resolves concrete provider images from this catalog during rollout.

## Example Usage

```typescript
import { SyncAcquireResponseHorizonHostImage } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseHorizonHostImage = {
  channel: "<value>",
  version: "<value>",
};
```

## Fields

| Field                                                    | Type                                                     | Required                                                 | Description                                              |
| -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- |
| `aws`                                                    | *models.SyncAcquireResponseHorizonHostImageAwsUnion*     | :heavy_minus_sign:                                       | N/A                                                      |
| `azure`                                                  | *models.SyncAcquireResponseHorizonHostImageAzureUnion*   | :heavy_minus_sign:                                       | N/A                                                      |
| `channel`                                                | *string*                                                 | :heavy_check_mark:                                       | Logical image channel, such as prod, staging, or canary. |
| `gcp`                                                    | *models.SyncAcquireResponseHorizonHostImageGcpUnion*     | :heavy_minus_sign:                                       | N/A                                                      |
| `version`                                                | *string*                                                 | :heavy_check_mark:                                       | Published image catalog version.                         |