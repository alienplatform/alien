# TerraformOutputs

Outputs from a Terraform package build.

## Example Usage

```typescript
import { TerraformOutputs } from "@alienplatform/platform-api/models";

let value: TerraformOutputs = {
  modules: {
    "key": {
      downloadUrl: "https://jittery-conservation.info/",
      filename: "example.file",
      shasum: "<value>",
      size: 266659,
      source: "<value>",
      target: "<value>",
    },
  },
  provider: {
    gpgPublicKey: {
      asciiArmor: "<value>",
      keyId: "<id>",
    },
    platforms: {},
    source: "<value>",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `modules`                                                                          | Record<string, [models.DeploymentInfoModules](../models/deploymentinfomodules.md)> | :heavy_check_mark:                                                                 | Module registry artifacts by Terraform target.                                     |
| `provider`                                                                         | [models.DeploymentInfoProvider](../models/deploymentinfoprovider.md)               | :heavy_check_mark:                                                                 | Terraform provider registry outputs.                                               |