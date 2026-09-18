# DeploymentEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { DeploymentEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: DeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "actual-metabolite.biz",
  os: "Windows Phone",
  platform: "local",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `arch`                                                                 | *string*                                                               | :heavy_check_mark:                                                     | Architecture (e.g., "x86_64", "aarch64")                               |
| `hostname`                                                             | *string*                                                               | :heavy_check_mark:                                                     | Hostname of the machine running the deployment                         |
| `os`                                                                   | *string*                                                               | :heavy_check_mark:                                                     | Operating system (e.g., "linux", "macos", "windows")                   |
| `platform`                                                             | [models.DeploymentPlatformLocal](../models/deploymentplatformlocal.md) | :heavy_check_mark:                                                     | N/A                                                                    |