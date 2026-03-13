# SyncAcquireResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncAcquireResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: SyncAcquireResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                            | Type                                                                                             | Required                                                                                         | Description                                                                                      |
| ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| `serviceAccountEmail`                                                                            | *string*                                                                                         | :heavy_check_mark:                                                                               | Service account email for management roles                                                       |
| `platform`                                                                                       | [models.SyncAcquireResponseConfigPlatformGcp](../models/syncacquireresponseconfigplatformgcp.md) | :heavy_check_mark:                                                                               | N/A                                                                                              |