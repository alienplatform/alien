# ManagerManagementConfigsGcp

## Example Usage

```typescript
import { ManagerManagementConfigsGcp } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigsGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                          | Type                                                                                           | Required                                                                                       | Description                                                                                    |
| ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                          | *string*                                                                                       | :heavy_check_mark:                                                                             | Service account email for management roles                                                     |
| `platform`                                                                                     | [models.ManagerManagementConfigsPlatformGcp](../models/managermanagementconfigsplatformgcp.md) | :heavy_check_mark:                                                                             | N/A                                                                                            |