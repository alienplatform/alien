# DeploymentSetupConfigInput

## Example Usage

```typescript
import { DeploymentSetupConfigInput } from "@alienplatform/platform-api/models";

let value: DeploymentSetupConfigInput = {};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `metadata`                                                                   | Record<string, *any*>                                                        | :heavy_minus_sign:                                                           | N/A                                                                          |
| `policy`                                                                     | [models.Policy](../models/policy.md)                                         | :heavy_minus_sign:                                                           | N/A                                                                          |
| `environmentVariables`                                                       | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[] | :heavy_minus_sign:                                                           | N/A                                                                          |
| `publicSubdomain`                                                            | *string*                                                                     | :heavy_minus_sign:                                                           | Operator-pinned deployment subdomain for this setup token.                   |