# SyncAcquireResponseManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncAcquireResponseManagementConfigAws } from "@aliendotdev/platform-api/models";

let value: SyncAcquireResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `managingRoleArn`                                                                                | *string*                                                                                         | :heavy_check_mark:                                                                               | The managing AWS IAM role ARN that can assume cross-account roles                                |
| `platform`                                                                                       | [models.SyncAcquireResponseConfigPlatformAws](../models/syncacquireresponseconfigplatformaws.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |