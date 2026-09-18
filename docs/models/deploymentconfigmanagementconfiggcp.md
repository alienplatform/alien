# DeploymentConfigManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { DeploymentConfigManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: DeploymentConfigManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `serviceAccountEmail`                                                          | *string*                                                                       | :heavy_check_mark:                                                             | Service account email for management roles                                     |
| `platform`                                                                     | [models.DeploymentConfigPlatformGcp](../models/deploymentconfigplatformgcp.md) | :heavy_check_mark:                                                             | N/A                                                                            |