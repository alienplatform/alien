# ManagerManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagerManagementConfigAws } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `managingRoleArn`                                                 | *string*                                                          | :heavy_check_mark:                                                | The managing AWS IAM role ARN that can assume cross-account roles |
| `platform`                                                        | [models.ManagerPlatformAws](../models/managerplatformaws.md)      | :heavy_check_mark:                                                | N/A                                                               |