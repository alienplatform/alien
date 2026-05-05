# ManagementConfigAws

AWS management configuration

## Example Usage

```typescript
import { ManagementConfigAws } from "@alienplatform/manager-api/models";

let value: ManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                             | Type                                                              | Required                                                          | Description                                                       |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| `managingRoleArn`                                                 | *string*                                                          | :heavy_check_mark:                                                | The managing AWS IAM role ARN that can assume cross-account roles |
| `platform`                                                        | *"aws"*                                                           | :heavy_check_mark:                                                | N/A                                                               |