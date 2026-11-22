# DeploymentInstallationTerraform

## Example Usage

```typescript
import { DeploymentInstallationTerraform } from "@alienplatform/platform-api/models";

let value: DeploymentInstallationTerraform = {
  target: "eks",
  hcl: "<value>",
  isAzureTarget: false,
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `target`                                                                         | [models.DeploymentInstallationTarget](../models/deploymentinstallationtarget.md) | :heavy_check_mark:                                                               | N/A                                                                              |
| `hcl`                                                                            | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `moduleDownloadUrl`                                                              | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `isAzureTarget`                                                                  | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |