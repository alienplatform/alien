# DebugSessionDeploymentEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { DebugSessionDeploymentEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: DebugSessionDeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "whimsical-offset.net",
  os: "Chrome OS",
  platform: "local",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `arch`                                                                                         | *string*                                                                                       | :heavy_check_mark:                                                                             | Architecture (e.g., "x86_64", "aarch64")                                                       |
| `hostname`                                                                                     | *string*                                                                                       | :heavy_check_mark:                                                                             | Hostname of the machine running the deployment                                                 |
| `os`                                                                                           | *string*                                                                                       | :heavy_check_mark:                                                                             | Operating system (e.g., "linux", "macos", "windows")                                           |
| `platform`                                                                                     | [models.DebugSessionDeploymentPlatformLocal](../models/debugsessiondeploymentplatformlocal.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |
