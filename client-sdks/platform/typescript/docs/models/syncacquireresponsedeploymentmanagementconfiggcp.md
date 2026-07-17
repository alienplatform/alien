# SyncAcquireResponseDeploymentManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncAcquireResponseDeploymentManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseDeploymentManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                                | Type                                                                                                                 | Required                                                                                                             | Description                                                                                                          |
| -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                                                | *string*                                                                                                             | :heavy_check_mark:                                                                                                   | Service account email for management roles                                                                           |
| `platform`                                                                                                           | [models.SyncAcquireResponseDeploymentConfigPlatformGcp](../models/syncacquireresponsedeploymentconfigplatformgcp.md) | :heavy_check_mark:                                                                                                   | N/A                                                                                                                  |