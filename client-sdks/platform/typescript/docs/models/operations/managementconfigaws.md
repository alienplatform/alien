# ManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagementConfigAws } from "@aliendotdev/platform-api/models/operations";

let value: ManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `managingRoleArn`                                                                          | *string*                                                                                   | :heavy_check_mark:                                                                         | The managing AWS IAM role ARN that can assume cross-account roles                          |
| `platform`                                                                                 | [operations.CreateManagerPlatformAws](../../models/operations/createmanagerplatformaws.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |