# DeploymentInstallationCommandVariants

## Example Usage

```typescript
import { DeploymentInstallationCommandVariants } from "@alienplatform/platform-api/models";

let value: DeploymentInstallationCommandVariants = {
  linux: {
    command: "<value>",
    install: "<value>",
  },
  mac: {
    command: "<value>",
    install: "<value>",
  },
  windows: {
    command: "<value>",
    install: "<value>",
  },
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `linux`                                                                            | [models.DeploymentInstallationLinux](../models/deploymentinstallationlinux.md)     | :heavy_check_mark:                                                                 | N/A                                                                                |
| `mac`                                                                              | [models.DeploymentInstallationMac](../models/deploymentinstallationmac.md)         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `windows`                                                                          | [models.DeploymentInstallationWindows](../models/deploymentinstallationwindows.md) | :heavy_check_mark:                                                                 | N/A                                                                                |