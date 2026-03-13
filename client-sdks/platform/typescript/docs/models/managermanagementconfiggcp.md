# ManagerManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagerManagementConfigGcp } from "@aliendotdev/platform-api/models";

let value: ManagerManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `serviceAccountEmail`                                        | *string*                                                     | :heavy_check_mark:                                           | Service account email for management roles                   |
| `platform`                                                   | [models.ManagerPlatformGcp](../models/managerplatformgcp.md) | :heavy_check_mark:                                           | N/A                                                          |