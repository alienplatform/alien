# UpdateDeploymentSetupPolicy

Editable part of a deployment link's setup config. Locked env vars and input values are preserved.

## Example Usage

```typescript
import { UpdateDeploymentSetupPolicy } from "@alienplatform/platform-api/models";

let value: UpdateDeploymentSetupPolicy = {
  policy: {
    allowedPlatforms: [],
    allowedSetupMethods: [],
  },
};
```

## Fields

| Field                                                                                                                                           | Type                                                                                                                                            | Required                                                                                                                                        | Description                                                                                                                                     |
| ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `returnUrl`                                                                                                                                     | *string*                                                                                                                                        | :heavy_minus_sign:                                                                                                                              | Vendor destination when leaving or completing setup. Null clears the destination.                                                               |
| `refreshUrl`                                                                                                                                    | *string*                                                                                                                                        | :heavy_minus_sign:                                                                                                                              | Vendor destination for expired, revoked, disabled or exhausted setup links. The vendor creates a fresh link there. Null clears the destination. |
| `policy`                                                                                                                                        | [models.DeploymentSetupPolicy](../models/deploymentsetuppolicy.md)                                                                              | :heavy_check_mark:                                                                                                                              | N/A                                                                                                                                             |
| `metadata`                                                                                                                                      | Record<string, *any*>                                                                                                                           | :heavy_minus_sign:                                                                                                                              | N/A                                                                                                                                             |