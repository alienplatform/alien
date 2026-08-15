# DeploymentInfoSetupConfigItem

## Example Usage

```typescript
import { DeploymentInfoSetupConfigItem } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfigItem = {
  item: "keys",
  source: {
    type: "built-in",
    definitionId: "customer-registry",
    version: "<value>",
    sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
  },
  required: true,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `item`                                                                                               | [models.DeploymentInfoSetupConfigItemEnum](../models/deploymentinfosetupconfigitemenum.md)           | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `source`                                                                                             | *models.DeploymentInfoSetupConfigSourceUnion*                                                        | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `required`                                                                                           | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `configuration`                                                                                      | [models.DeploymentInfoSetupConfigConfiguration](../models/deploymentinfosetupconfigconfiguration.md) | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |