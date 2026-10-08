# PackagesConfigPatchSetupResources

## Example Usage

```typescript
import { PackagesConfigPatchSetupResources } from "@alienplatform/platform-api/models";

let value: PackagesConfigPatchSetupResources = {
  tokenSecret: {
    key: "<key>",
    name: "<value>",
    prefix: "<value>",
    randomLength: 13795,
  },
  workloadReadAccess: {
    rules: [
      {
        apiGroup: "<value>",
        resources: [
          "<value 1>",
        ],
      },
    ],
    serviceAccountProfile: "<value>",
  },
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `tokenSecret`                                                                                      | [models.PackagesConfigPatchTokenSecret](../models/packagesconfigpatchtokensecret.md)               | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `workloadReadAccess`                                                                               | [models.PackagesConfigPatchWorkloadReadAccess](../models/packagesconfigpatchworkloadreadaccess.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |
