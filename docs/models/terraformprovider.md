# TerraformProvider

Terraform provider registry outputs.

## Example Usage

```typescript
import { TerraformProvider } from "@alienplatform/platform-api/models";

let value: TerraformProvider = {
  gpgPublicKey: {
    asciiArmor: "<value>",
    keyId: "<id>",
  },
  platforms: {},
  source: "<value>",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `gpgPublicKey`                                                                         | [models.DeploymentInfoGpgPublicKey](../models/deploymentinfogpgpublickey.md)           | :heavy_check_mark:                                                                     | GPG public key for Terraform provider signature verification                           |
| `platforms`                                                                            | Record<string, [models.DeploymentInfoPlatforms](../models/deploymentinfoplatforms.md)> | :heavy_check_mark:                                                                     | Provider packages for each target platform                                             |
| `source`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Terraform provider source (hostname/namespace/type, without scheme)                    |