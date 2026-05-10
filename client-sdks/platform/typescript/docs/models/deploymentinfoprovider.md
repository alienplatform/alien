# DeploymentInfoProvider

Terraform provider registry outputs.

## Example Usage

```typescript
import { DeploymentInfoProvider } from "@alienplatform/platform-api/models";

let value: DeploymentInfoProvider = {
  gpgPublicKey: {
    asciiArmor: "<value>",
    keyId: "<id>",
  },
  platforms: {
    "key": {
      downloadUrl: "https://scornful-bonnet.net",
      filename: "example.file",
      shasum: "<value>",
      shasumsSignatureUrl: "https://our-soybean.biz/",
      shasumsUrl: "https://passionate-pop.org/",
      size: 227657,
    },
  },
  source: "<value>",
};
```

## Fields

| Field                                                                                  | Type                                                                                   | Required                                                                               | Description                                                                            |
| -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- |
| `gpgPublicKey`                                                                         | [models.DeploymentInfoGpgPublicKey](../models/deploymentinfogpgpublickey.md)           | :heavy_check_mark:                                                                     | GPG public key for Terraform provider signature verification                           |
| `platforms`                                                                            | Record<string, [models.DeploymentInfoPlatforms](../models/deploymentinfoplatforms.md)> | :heavy_check_mark:                                                                     | Provider packages for each target platform                                             |
| `source`                                                                               | *string*                                                                               | :heavy_check_mark:                                                                     | Terraform provider source (hostname/namespace/type, without scheme)                    |