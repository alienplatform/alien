# TerraformOutputs

Outputs from a Terraform provider package build

## Example Usage

```typescript
import { TerraformOutputs } from "@aliendotdev/platform-api/models";

let value: TerraformOutputs = {
  gpgPublicKey: {
    asciiArmor: "<value>",
    keyId: "<id>",
  },
  platforms: {
    "key": {
      downloadUrl: "https://jittery-conservation.info/",
      filename: "example.file",
      shasum: "<value>",
      shasumsSignatureUrl: "https://imaginative-instance.biz/",
      shasumsUrl: "https://untidy-glider.info",
      size: 810166,
    },
  },
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `gpgPublicKey`                                                                         | [models.DeploymentInfoGpgPublicKey](../models/deploymentinfogpgpublickey.md)           | :heavy_check_mark:                                                                     | GPG public key for Terraform provider signature verification                           |
| `platforms`                                                                            | Record<string, [models.DeploymentInfoPlatforms](../models/deploymentinfoplatforms.md)> | :heavy_check_mark:                                                                     | Provider packages for each target platform                                             |