# PersistImportedDeploymentRequestManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { PersistImportedDeploymentRequestManagementConfigAws } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `managingRoleArn`                                                 | *string*                                                          | :heavy_check_mark:                                                | The managing AWS IAM role ARN that can assume cross-account roles |
| `platform`                                                        | *"aws"*                                                           | :heavy_check_mark:                                                | N/A                                                               |