# DeploymentStateEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { DeploymentStateEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: DeploymentStateEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `accountId`                                                                  | *string*                                                                     | :heavy_check_mark:                                                           | AWS account ID                                                               |
| `region`                                                                     | *string*                                                                     | :heavy_check_mark:                                                           | AWS region                                                                   |
| `platform`                                                                   | [models.DeploymentStatePlatformAws](../models/deploymentstateplatformaws.md) | :heavy_check_mark:                                                           | N/A                                                                          |