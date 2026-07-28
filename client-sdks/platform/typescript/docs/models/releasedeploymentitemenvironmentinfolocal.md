# ReleaseDeploymentItemEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { ReleaseDeploymentItemEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: ReleaseDeploymentItemEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "unused-overcoat.biz",
  os: "Linux",
  platform: "local",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `arch`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | Architecture (e.g., "x86_64", "aarch64")                                                     |
| `hostname`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Hostname of the machine running the deployment                                               |
| `os`                                                                                         | *string*                                                                                     | :heavy_check_mark:                                                                           | Operating system (e.g., "linux", "macos", "windows")                                         |
| `platform`                                                                                   | [models.ReleaseDeploymentItemPlatformLocal](../models/releasedeploymentitemplatformlocal.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |
