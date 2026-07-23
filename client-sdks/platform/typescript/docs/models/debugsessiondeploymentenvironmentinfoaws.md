# DebugSessionDeploymentEnvironmentInfoAws

AWS-specific environment information

## Example Usage

```typescript
import { DebugSessionDeploymentEnvironmentInfoAws } from "@alienplatform/platform-api/models";

let value: DebugSessionDeploymentEnvironmentInfoAws = {
  accountId: "<id>",
  region: "<value>",
  platform: "aws",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `accountId`                                                                                | *string*                                                                                   | :heavy_check_mark:                                                                         | AWS account ID                                                                             |
| `region`                                                                                   | *string*                                                                                   | :heavy_check_mark:                                                                         | AWS region                                                                                 |
| `platform`                                                                                 | [models.DebugSessionDeploymentPlatformAws](../models/debugsessiondeploymentplatformaws.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |
