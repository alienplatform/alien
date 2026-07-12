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
  visiblePackageTypes: [
    "cloudformation",
  ],
  visibleSetupMethods: [
    "manual",
  ],
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `activeRelease`                                                      | [models.ActiveRelease](../models/activerelease.md)                   | :heavy_check_mark:                                                   | N/A                                                                  |
| `visiblePackageTypes`                                                | [models.VisiblePackageType](../models/visiblepackagetype.md)[]       | :heavy_check_mark:                                                   | N/A                                                                  |
| `visibleSetupMethods`                                                | [models.DeploymentSetupMethod](../models/deploymentsetupmethod.md)[] | :heavy_check_mark:                                                   | N/A                                                                  |