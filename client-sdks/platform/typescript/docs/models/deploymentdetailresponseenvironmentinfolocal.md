# DeploymentDetailResponseEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { DeploymentDetailResponseEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: DeploymentDetailResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "official-wear.biz",
  os: "BeOS",
  platform: "local",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `arch`                                                                                             | *string*                                                                                           | :heavy_check_mark:                                                                                 | Architecture (e.g., "x86_64", "aarch64")                                                           |
| `hostname`                                                                                         | *string*                                                                                           | :heavy_check_mark:                                                                                 | Hostname of the machine running the agent                                                          |
| `os`                                                                                               | *string*                                                                                           | :heavy_check_mark:                                                                                 | Operating system (e.g., "linux", "macos", "windows")                                               |
| `platform`                                                                                         | [models.DeploymentDetailResponsePlatformLocal](../models/deploymentdetailresponseplatformlocal.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |