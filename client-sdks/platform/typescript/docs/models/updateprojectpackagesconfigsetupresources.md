# UpdateProjectPackagesConfigSetupResources

## Example Usage

```typescript
import { UpdateProjectPackagesConfigSetupResources } from "@alienplatform/platform-api/models";

let value: UpdateProjectPackagesConfigSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 698813,
  },
  workloadReadAccess: {
    rules: [],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `tokenSecret`                                                                                                      | [models.UpdateProjectPackagesConfigTokenSecret](../models/updateprojectpackagesconfigtokensecret.md)               | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |
| `workloadReadAccess`                                                                                               | [models.UpdateProjectPackagesConfigWorkloadReadAccess](../models/updateprojectpackagesconfigworkloadreadaccess.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |