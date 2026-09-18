# PackageProvider

Terraform provider registry outputs.

## Example Usage

```typescript
import { PackageProvider } from "@alienplatform/platform-api/models";

let value: PackageProvider = {
  gpgPublicKey: {
    asciiArmor: "<value>",
    keyId: "<id>",
  },
  platforms: {},
  source: "<value>",
};
```

## Fields

| Field                                                                    | Type                                                                     | Required                                                                 | Description                                                              |
| ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ | ------------------------------------------------------------------------ |
| `gpgPublicKey`                                                           | [models.PackageGpgPublicKey](../models/packagegpgpublickey.md)           | :heavy_check_mark:                                                       | GPG public key for Terraform provider signature verification             |
| `platforms`                                                              | Record<string, [models.PackagePlatforms](../models/packageplatforms.md)> | :heavy_check_mark:                                                       | Provider packages for each target platform                               |
| `source`                                                                 | *string*                                                                 | :heavy_check_mark:                                                       | Terraform provider source (hostname/namespace/type, without scheme)      |