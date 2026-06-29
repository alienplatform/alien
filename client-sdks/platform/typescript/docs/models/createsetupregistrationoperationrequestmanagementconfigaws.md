# CreateSetupRegistrationOperationRequestManagementConfigAws

AWS management configuration extracted from stack settings

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestManagementConfigAws } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestManagementConfigAws = {
  managingRoleArn: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `managingRoleArn`                                                                                                            | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | The managing AWS IAM role ARN that can assume cross-account roles                                                            |
| `platform`                                                                                                                   | [models.CreateSetupRegistrationOperationRequestPlatformAws](../models/createsetupregistrationoperationrequestplatformaws.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |