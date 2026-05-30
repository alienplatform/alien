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
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `allowedPlatforms`                                                                           | [models.AllowedPlatform](../models/allowedplatform.md)[]                                     | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `allowedSetupMethods`                                                                        | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[]                         | :heavy_check_mark:                                                                           | N/A                                                                                          |
| `stackSettings`                                                                              | [models.DeploymentSetupStackSettingsPolicy](../models/deploymentsetupstacksettingspolicy.md) | :heavy_minus_sign:                                                                           | N/A                                                                                          |