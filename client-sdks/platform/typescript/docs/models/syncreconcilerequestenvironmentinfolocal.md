# SyncReconcileRequestEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { SyncReconcileRequestEnvironmentInfoLocal } from "@aliendotdev/platform-api/models";

let value: SyncReconcileRequestEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "recent-requirement.name",
  os: "Symbian",
  platform: "local",
};
```

## Fields

| Field                                                                                      | Type                                                                                       | Required                                                                                   | Description                                                                                |
| ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------ |
| `arch`                                                                                     | *string*                                                                                   | :heavy_check_mark:                                                                         | Architecture (e.g., "x86_64", "aarch64")                                                   |
| `hostname`                                                                                 | *string*                                                                                   | :heavy_check_mark:                                                                         | Hostname of the machine running the agent                                                  |
| `os`                                                                                       | *string*                                                                                   | :heavy_check_mark:                                                                         | Operating system (e.g., "linux", "macos", "windows")                                       |
| `platform`                                                                                 | [models.SyncReconcileRequestPlatformLocal](../models/syncreconcilerequestplatformlocal.md) | :heavy_check_mark:                                                                         | N/A                                                                                        |