# APIKeyDeploymentSetupConfigItem

## Example Usage

```typescript
import { APIKeyDeploymentSetupConfigItem } from "@alienplatform/platform-api/models";

let value: APIKeyDeploymentSetupConfigItem = {
  item: "deployment",
  source: {
    type: "built-in",
    definitionId: "customer-key",
    version: "<value>",
    sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
  required: false,
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `item`                                                                                                   | [models.APIKeyDeploymentSetupConfigItemEnum](../models/apikeydeploymentsetupconfigitemenum.md)           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `source`                                                                                                 | *models.APIKeyDeploymentSetupConfigSourceUnion*                                                          | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `required`                                                                                               | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `configuration`                                                                                          | [models.APIKeyDeploymentSetupConfigConfiguration](../models/apikeydeploymentsetupconfigconfiguration.md) | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |