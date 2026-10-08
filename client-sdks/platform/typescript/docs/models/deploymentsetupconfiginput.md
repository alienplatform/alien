# DeploymentSetupConfigInput

## Example Usage

```typescript
import { DeploymentSetupConfigInput } from "@alienplatform/platform-api/models";

let value: DeploymentSetupConfigInput = {
  validatedReleaseSelection: {
    releaseId: "rel_WbhQgksrawSKIpEN0NAssHX9",
    releaseChannel: "<value>",
    platform: "aws",
  },
};
```

## Fields

| Field                                                                                                                          | Type                                                                                                                           | Required                                                                                                                       | Description                                                                                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `metadata`                                                                                                                     | Record<string, *any*>                                                                                                          | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
| `policy`                                                                                                                       | [models.Policy](../models/policy.md)                                                                                           | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
| `environmentVariables`                                                                                                         | [models.EnvironmentVariableConfig](../models/environmentvariableconfig.md)[]                                                   | :heavy_minus_sign:                                                                                                             | N/A                                                                                                                            |
| `validatedReleaseSelection`                                                                                                    | [models.DeploymentSetupConfigInputValidatedReleaseSelection](../models/deploymentsetupconfiginputvalidatedreleaseselection.md) | :heavy_minus_sign:                                                                                                             | Immutable Release whose schema validated first-party inputs for this platform and channel.                                     |
| `publicSubdomain`                                                                                                              | *string*                                                                                                                       | :heavy_minus_sign:                                                                                                             | Operator-pinned deployment subdomain for this setup token.                                                                     |