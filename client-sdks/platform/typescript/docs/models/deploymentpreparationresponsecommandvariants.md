# DeploymentPreparationResponseCommandVariants

## Example Usage

```typescript
import { DeploymentPreparationResponseCommandVariants } from "@alienplatform/platform-api/models";

let value: DeploymentPreparationResponseCommandVariants = {
  linux: {
    install: "<value>",
    command: "<value>",
  },
  mac: {
    install: "<value>",
    command: "<value>",
  },
  windows: {
    install: "<value>",
    command: "<value>",
  },
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `linux`                                                                                          | [models.DeploymentPreparationResponseLinux](../models/deploymentpreparationresponselinux.md)     | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `mac`                                                                                            | [models.DeploymentPreparationResponseMac](../models/deploymentpreparationresponsemac.md)         | :heavy_check_mark:                                                                               | N/A                                                                                              |
| `windows`                                                                                        | [models.DeploymentPreparationResponseWindows](../models/deploymentpreparationresponsewindows.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |