# CommandDeploymentInfoEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { CommandDeploymentInfoEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: CommandDeploymentInfoEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "idealistic-disposer.info",
  os: "iOS",
  platform: "local",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `arch`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | Architecture (e.g., "x86_64", "aarch64")                                                     |
| `hostname`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Hostname of the machine running the deployment                                               |
| `os`                                                                                         | *string*                                                                                     | :heavy_check_mark:                                                                           | Operating system (e.g., "linux", "macos", "windows")                                         |
| `platform`                                                                                   | [models.CommandDeploymentInfoPlatformLocal](../models/commanddeploymentinfoplatformlocal.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |