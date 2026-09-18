# PersistImportedDeploymentRequestEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { PersistImportedDeploymentRequestEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: PersistImportedDeploymentRequestEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "well-off-maintainer.net",
  os: "BeOS",
  platform: "local",
};
```

## Fields

| Field                                                                                                              | Type                                                                                                               | Required                                                                                                           | Description                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `arch`                                                                                                             | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Architecture (e.g., "x86_64", "aarch64")                                                                           |
| `hostname`                                                                                                         | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Hostname of the machine running the deployment                                                                     |
| `os`                                                                                                               | *string*                                                                                                           | :heavy_check_mark:                                                                                                 | Operating system (e.g., "linux", "macos", "windows")                                                               |
| `platform`                                                                                                         | [models.PersistImportedDeploymentRequestPlatformLocal](../models/persistimporteddeploymentrequestplatformlocal.md) | :heavy_check_mark:                                                                                                 | N/A                                                                                                                |