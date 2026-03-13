# CommandDeploymentInfoEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { CommandDeploymentInfoEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: CommandDeploymentInfoEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                    | Type                                                                                     | Required                                                                                 | Description                                                                              |
| ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| `accountId`                                                                              | *string*                                                                                 | :heavy_check_mark:                                                                       | AWS account ID                                                                           |
| `region`                                                                                 | *string*                                                                                 | :heavy_check_mark:                                                                       | AWS region                                                                               |
| `platform`                                                                               | [models.CommandDeploymentInfoPlatformAws](../models/commanddeploymentinfoplatformaws.md) | :heavy_check_mark:                                                                       | N/A                                                                                      |