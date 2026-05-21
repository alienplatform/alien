# SyncListResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncListResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: SyncListResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                          | Type                                                                                                           | Required                                                                                                       | Description                                                                                                    |
| -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                                          | *string*                                                                                                       | :heavy_check_mark:                                                                                             | Service account email for management roles                                                                     |
| `platform`                                                                                                     | [models.SyncListResponseManagementConfigPlatformGcp](../models/synclistresponsemanagementconfigplatformgcp.md) | :heavy_check_mark:                                                                                             | N/A                                                                                                            |