# OutputsTerraform

Outputs from a Terraform provider package build

## Example Usage

```typescript
import { OutputsTerraform } from "@aliendotdev/platform-api/models";

let value: OutputsTerraform = {
  gpgPublicKey: {
    asciiArmor: "<value>",
    keyId: "<id>",
  },
  platforms: {},
  type: "terraform",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `gpgPublicKey`                                                           | [models.PackageGpgPublicKey](../models/packagegpgpublickey.md)           | :heavy_check_mark:                                                       | GPG public key for Terraform provider signature verification             |
| `platforms`                                                              | Record<string, [models.PackagePlatforms](../models/packageplatforms.md)> | :heavy_check_mark:                                                       | Provider packages for each target platform                               |
| `type`                                                                   | [models.OutputsTypeTerraform](../models/outputstypeterraform.md)         | :heavy_check_mark:                                                       | N/A                                                                      |