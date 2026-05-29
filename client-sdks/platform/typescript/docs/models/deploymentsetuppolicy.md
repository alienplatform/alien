# DeploymentSetupPolicy

## Example Usage

```typescript
import { DeploymentSetupPolicy } from "@alienplatform/platform-api/models";

let value: DeploymentSetupPolicy = {
  allowedPlatforms: [
    "test",
  ],
  allowedSetupMethods: [
    "cloudformation",
  ],
  release: {
    mode: "fixed",
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `allowedPlatforms`                                                                           | [models.AllowedPlatform](../models/allowedplatform.md)[]                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `allowedSetupMethods`                                                                        | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[]                         | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `deploymentName`                                                                             | *models.DeploymentSetupDeploymentNamePolicy*                                                 | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `release`                                                                                    | *models.DeploymentSetupReleasePolicy*                                                        | :heavy_minus_sign:                                                                           | N/A                                                                                          |
| `stackSettings`                                                                              | [models.DeploymentSetupStackSettingsPolicy](../models/deploymentsetupstacksettingspolicy.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |