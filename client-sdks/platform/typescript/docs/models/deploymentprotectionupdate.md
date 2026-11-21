# DeploymentProtectionUpdate

## Example Usage

```typescript
import { DeploymentProtectionUpdate } from "@alienplatform/platform-api/models";

let value: DeploymentProtectionUpdate = {
  setupMethod: "terraform",
  fingerprint: "<value>",
  releaseId: "<id>",
  templateUrl: "https://gracious-account.name/",
  steps: [],
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `setupMethod`                                                                                      | [models.DeploymentProtectionUpdateSetupMethod](../models/deploymentprotectionupdatesetupmethod.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `fingerprint`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `releaseId`                                                                                        | *string*                                                                                           | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `templateUrl`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | The published signed template or archive                                                           |
| `steps`                                                                                            | [models.DeploymentProtectionUpdateStep](../models/deploymentprotectionupdatestep.md)[]             | :heavy_check_mark:                                                                                 | N/A                                                                                                |