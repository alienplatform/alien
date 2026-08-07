# DeploymentLinkSetupResponse

## Example Usage

```typescript
import { DeploymentLinkSetupResponse } from "@alienplatform/platform-api/models";

let value: DeploymentLinkSetupResponse = {
  activeRelease: {
    id: "rel_WbhQgksrawSKIpEN0NAssHX9",
    version: "<value>",
    stack: {},
  },
  supportedPlatforms: [],
  setupItems: [
    "storage",
  ],
  visiblePackageTypes: [],
  visibleSetupMethods: [],
  setupPackagesStatus: "preparing",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `activeRelease`                                                                                                    | [models.ActiveRelease](../models/activerelease.md)                                                                 | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `supportedPlatforms`                                                                                               | [models.DeploymentLinkSetupResponseSupportedPlatform](../models/deploymentlinksetupresponsesupportedplatform.md)[] | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `setupItems`                                                                                                       | [models.DeploymentLinkSetupResponseSetupItem](../models/deploymentlinksetupresponsesetupitem.md)[]                 | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `visiblePackageTypes`                                                                                              | [models.VisiblePackageType](../models/visiblepackagetype.md)[]                                                     | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `visibleSetupMethods`                                                                                              | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[]                                               | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `setupPackagesStatus`                                                                                              | [models.SetupPackagesStatus](../models/setuppackagesstatus.md)                                                     | :heavy_check_mark:                                                                                                 | Whether at least one complete automated setup package is ready across all selected setup items.                    |