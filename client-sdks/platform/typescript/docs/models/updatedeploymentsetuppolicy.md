# UpdateDeploymentSetupPolicy

Editable part of a deployment link's setup config. Locked env vars and input values are preserved.

## Example Usage

```typescript
import { UpdateDeploymentSetupPolicy } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentSetupPolicy = {
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [
      "google-oauth",
    ],
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `policy`                                                           | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `metadata`                                                         | Record<string, *any*>                                              | :heavy_minus_sign:                                                 | N/A                                                                |
