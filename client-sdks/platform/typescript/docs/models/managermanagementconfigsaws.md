# ManagerManagementConfigsAws

## Example Usage

```typescript
import { ManagerManagementConfigsAws } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigsAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `managingRoleArn`                                                                              | *string*                                                                                       | :heavy_check_mark:                                                                             | The managing AWS IAM role ARN that can assume cross-account roles                              |
| `platform`                                                                                     | [models.ManagerManagementConfigsPlatformAws](../models/managermanagementconfigsplatformaws.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |