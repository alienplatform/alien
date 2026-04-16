# DeploymentInfoCli

## Example Usage

```typescript
import { DeploymentInfoCli } from "@alienplatform/platform-api/models";

let value: DeploymentInfoCli = {
  status: "ready",
  installScripts: {
    windows: "https://mild-inspection.net",
    mac: "https://biodegradable-issue.biz/",
    linux: "https://scared-illusion.biz",
  },
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `status`                                             | [models.CliStatus](../models/clistatus.md)           | :heavy_check_mark:                                   | Status of a package build                            |
| `version`                                            | *string*                                             | :heavy_minus_sign:                                   | N/A                                                  |
| `outputs`                                            | [models.CliOutputs](../models/clioutputs.md)         | :heavy_minus_sign:                                   | Outputs from a CLI package build                     |
| `error`                                              | *any*                                                | :heavy_minus_sign:                                   | N/A                                                  |
| `installScripts`                                     | [models.InstallScripts](../models/installscripts.md) | :heavy_check_mark:                                   | Install script URLs for each OS                      |