# SyncListResponseManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncListResponseManagementConfigAws } from "@alienplatform/platform-api/models";

let value: SyncListResponseManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `managingRoleArn`                                                                                              | *string*                                                                                                       | :heavy_check_mark:                                                                                             | The managing AWS IAM role ARN that can assume cross-account roles                                              |
| `platform`                                                                                                     | [models.SyncListResponseManagementConfigPlatformAws](../models/synclistresponsemanagementconfigplatformaws.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |