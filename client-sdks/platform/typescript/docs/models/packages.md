# Packages

## Example Usage

```typescript
import { Packages } from "@alienplatform/platform-api/models";

let value: Packages = {
  ready: false,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `ready`                                                                          | *boolean*                                                                        | :heavy_check_mark:                                                               | True if all enabled packages are ready for deployment                            |
| `cli`                                                                            | [models.DeploymentInfoCli](../models/deploymentinfocli.md)                       | :heavy_minus_sign:                                                               | N/A                                                                              |
| `cloudformation`                                                                 | [models.DeploymentInfoCloudformation](../models/deploymentinfocloudformation.md) | :heavy_minus_sign:                                                               | N/A                                                                              |
| `terraform`                                                                      | [models.DeploymentInfoTerraform](../models/deploymentinfoterraform.md)           | :heavy_minus_sign:                                                               | N/A                                                                              |