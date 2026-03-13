# SyncReconcileResponseEnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { SyncReconcileResponseEnvironmentInfoLocal } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseEnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "miserable-eyeliner.com",
  os: "Chrome OS",
  platform: "local",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `arch`                                                                                       | *string*                                                                                     | :heavy_check_mark:                                                                           | Architecture (e.g., "x86_64", "aarch64")                                                     |
| `hostname`                                                                                   | *string*                                                                                     | :heavy_check_mark:                                                                           | Hostname of the machine running the agent                                                    |
| `os`                                                                                         | *string*                                                                                     | :heavy_check_mark:                                                                           | Operating system (e.g., "linux", "macos", "windows")                                         |
| `platform`                                                                                   | [models.SyncReconcileResponsePlatformLocal](../models/syncreconcileresponseplatformlocal.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |