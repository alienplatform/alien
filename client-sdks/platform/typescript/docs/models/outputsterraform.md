# OutputsTerraform

Outputs from a Terraform package build.

## Example Usage

```typescript
import { OutputsTerraform } from "@alienplatform/platform-api/models";

let value: OutputsTerraform = {
  modules: {},
  provider: {
    gpgPublicKey: {
      asciiArmor: "<value>",
      keyId: "<id>",
    },
    platforms: {},
    source: "<value>",
  },
  type: "terraform",
};
```

## Fields

| Field                                                                | Type                                                                 | Required                                                             | Description                                                          |
| -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `modules`                                                            | Record<string, [models.PackageModules](../models/packagemodules.md)> | :heavy_check_mark:                                                   | Module registry artifacts by Terraform target.                       |
| `provider`                                                           | [models.PackageProvider](../models/packageprovider.md)               | :heavy_check_mark:                                                   | Terraform provider registry outputs.                                 |
| `type`                                                               | [models.OutputsTypeTerraform](../models/outputstypeterraform.md)     | :heavy_check_mark:                                                   | N/A                                                                  |