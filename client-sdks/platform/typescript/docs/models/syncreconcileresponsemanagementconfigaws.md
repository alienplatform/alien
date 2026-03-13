# SyncReconcileResponseManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncReconcileResponseManagementConfigAws } from "@aliendotdev/platform-api/models";

let value: SyncReconcileResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `managingRoleArn`                                                 | *string*                                                          | :heavy_check_mark:                                                | The managing AWS IAM role ARN that can assume cross-account roles |
| `platform`                                                        | [models.TargetPlatformAws](../models/targetplatformaws.md)        | :heavy_check_mark:                                                | N/A                                                               |