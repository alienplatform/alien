# DeploymentInfoManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { DeploymentInfoManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: DeploymentInfoManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                        | Type                                                         | Required                                                     | Description                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ | ------------------------------------------------------------ |
| `serviceAccountEmail`                                        | *string*                                                     | :heavy_check_mark:                                           | Service account email for management roles                   |
| `platform`                                                   | [models.TargetsPlatformGcp](../models/targetsplatformgcp.md) | :heavy_check_mark:                                           | N/A                                                          |