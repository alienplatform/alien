# SyncReconcileResponseManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { SyncReconcileResponseManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: SyncReconcileResponseManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `serviceAccountEmail`                                      | *string*                                                   | :heavy_check_mark:                                         | Service account email for management roles                 |
| `platform`                                                 | [models.TargetPlatformGcp](../models/targetplatformgcp.md) | :heavy_check_mark:                                         | N/A                                                        |