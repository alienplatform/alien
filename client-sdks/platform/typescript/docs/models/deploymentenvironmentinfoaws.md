# DeploymentEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { DeploymentEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: DeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `accountId`                                                        | *string*                                                           | :heavy_check_mark:                                                 | AWS account ID                                                     |
| `region`                                                           | *string*                                                           | :heavy_check_mark:                                                 | AWS region                                                         |
| `platform`                                                         | [models.DeploymentPlatformAws](../models/deploymentplatformaws.md) | :heavy_check_mark:                                                 | N/A                                                                |