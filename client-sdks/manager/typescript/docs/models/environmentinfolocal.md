# EnvironmentInfoLocal

Local platform environment information

## Example Usage

```typescript
import { EnvironmentInfoLocal } from "@alienplatform/manager-api/models";

let value: EnvironmentInfoLocal = {
  arch: "<value>",
  hostname: "pertinent-spear.com",
  os: "Windows Phone",
  platform: "local",
};
```

## Fields

| Field                                                | Type                                                 | Required                                             | Description                                          |
| ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- | ---------------------------------------------------- |
| `arch`                                               | *string*                                             | :heavy_check_mark:                                   | Architecture (e.g., "x86_64", "aarch64")             |
| `hostname`                                           | *string*                                             | :heavy_check_mark:                                   | Hostname of the machine running the agent            |
| `os`                                                 | *string*                                             | :heavy_check_mark:                                   | Operating system (e.g., "linux", "macos", "windows") |
| `platform`                                           | *"local"*                                            | :heavy_check_mark:                                   | N/A                                                  |