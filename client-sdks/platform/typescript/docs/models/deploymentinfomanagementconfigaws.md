# DeploymentInfoManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { DeploymentInfoManagementConfigAws } from "@alienplatform/platform-api/models";

let value: DeploymentInfoManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `managingRoleArn`                                                 | *string*                                                          | :heavy_check_mark:                                                | The managing AWS IAM role ARN that can assume cross-account roles |
| `platform`                                                        | *"aws"*                                                           | :heavy_check_mark:                                                | N/A                                                               |