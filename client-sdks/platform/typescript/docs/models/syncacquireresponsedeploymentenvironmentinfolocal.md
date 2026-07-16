# SyncAcquireResponseDeploymentEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "leading-graffiti.com",
  os: "Linux",
  platform: "local",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `arch`                                                                                                       | *string*                                                                                                     | :heavy_check_mark:                                                                                           | Architecture (e.g., "x86_64", "aarch64")                                                                     |
| `hostname`                                                                                                   | *string*                                                                                                     | :heavy_check_mark:                                                                                           | Hostname of the machine running the deployment                                                               |
| `os`                                                                                                         | *string*                                                                                                     | :heavy_check_mark:                                                                                           | Operating system (e.g., "linux", "macos", "windows")                                                         |
| `platform`                                                                                                   | [models.SyncAcquireResponseDeploymentPlatformLocal](../models/syncacquireresponsedeploymentplatformlocal.md) | :heavy_check_mark:                                                                                           | N/A                                                                                                          |