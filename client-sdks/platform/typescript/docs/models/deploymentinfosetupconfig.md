# DeploymentInfoSetupConfig

## Example Usage

```typescript
import { DeploymentInfoSetupConfig } from "@alienplatform/platform-api/models";

let value: DeploymentInfoSetupConfig = {
  metadata: {
    "key": "<value>",
    "key1": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [],
  },
  environmentVariables: [],
  items: [
    {
      item: "registry",
      source: {
        type: "built-in",
        definitionId: "customer-ai",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: true,
    },
  ],
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `returnUrl`                                                                                                                                     | *string*                                                                                                                                        | :heavy_minus_sign:                                                                                                                              | Vendor destination when leaving or completing setup. Null clears the destination.                                                               |
| `refreshUrl`                                                                                                                                    | *string*                                                                                                                                        | :heavy_minus_sign:                                                                                                                              | Vendor destination for expired, revoked, disabled or exhausted setup links. The vendor creates a fresh link there. Null clears the destination. |
| `metadata`                                                                                                                                      | Record<string, *any*>                                                                                                                           | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `policy`                                                                                                                                        | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                                                              | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `environmentVariables`                                                                                                                          | [models.DeploymentInfoSetupConfigEnvironmentVariable](../models/deploymentinfosetupconfigenvironmentvariable.md)[]                              | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `items`                                                                                                                                         | [models.DeploymentInfoSetupConfigItem](../models/deploymentinfosetupconfigitem.md)[]                                                            | :heavy_minus_sign:                                                                                                                              | N/A                                                                                                                                             |
| `inputs`                                                                                                                                        | [models.DeploymentInfoSetupConfigInput](../models/deploymentinfosetupconfiginput.md)[]                                                          | :heavy_minus_sign:                                                                                                                              | N/A                                                                                                                                             |
| `inputValues`                                                                                                                                   | [models.ResolvedStackInputSummary](../models/resolvedstackinputsummary.md)[]                                                                    | :heavy_minus_sign:                                                                                                                              | N/A                                                                                                                                             |