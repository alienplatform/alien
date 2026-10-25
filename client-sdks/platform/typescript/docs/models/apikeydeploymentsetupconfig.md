# APIKeyDeploymentSetupConfig

## Example Usage

```typescript
import { APIKeyDeploymentSetupConfig } from "@alienplatform/platform-api/models";

let value: APIKeyDeploymentSetupConfig = {
  metadata: {
    "key": "<value>",
  },
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [],
  },
  environmentVariables: [
    {
      name: "<value>",
      type: "plain",
      targetResources: [],
    },
  ],
  items: [
    {
      item: "bucket",
      source: {
        type: "built-in",
        definitionId: "customer-sandbox",
        version: "<value>",
        sourceReleaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
      },
      required: false,
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
| `environmentVariables`                                                                                                                          | [models.APIKeyDeploymentSetupEnvironmentVariable](../models/apikeydeploymentsetupenvironmentvariable.md)[]                                      | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `items`                                                                                                                                         | [models.APIKeyDeploymentSetupConfigItem](../models/apikeydeploymentsetupconfigitem.md)[]                                                        | :heavy_minus_sign:                                                                                                                              | N/A                                                                                                                                             |