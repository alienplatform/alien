# NewDeploymentRequestEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { NewDeploymentRequestEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: NewDeploymentRequestEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `accountId`                                                                            | *string*                                                                               | :heavy_check_mark:                                                                     | AWS account ID                                                                         |
| `region`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | AWS region                                                                             |
| `platform`                                                                             | [models.NewDeploymentRequestPlatformAws](../models/newdeploymentrequestplatformaws.md) | :heavy_check_mark:                                                                     | N/A                                                                                    |