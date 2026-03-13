# DeploymentListItemResponseEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { DeploymentListItemResponseEnvironmentInfoAws } from "@aliendotdev/platform-api/models";

let value: DeploymentListItemResponseEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `accountId`                                                                                        | *string*                                                                                           | :heavy_check_mark:                                                                                 | AWS account ID                                                                                     |
| `region`                                                                                           | *string*                                                                                           | :heavy_check_mark:                                                                                 | AWS region                                                                                         |
| `platform`                                                                                         | [models.DeploymentListItemResponsePlatformAws](../models/deploymentlistitemresponseplatformaws.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |