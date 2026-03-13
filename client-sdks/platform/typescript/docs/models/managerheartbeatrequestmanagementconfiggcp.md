# ManagerHeartbeatRequestManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { ManagerHeartbeatRequestManagementConfigGcp } from "@aliendotdev/platform-api/models";

let value: ManagerHeartbeatRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                        | Type                                                                                         | Required                                                                                     | Description                                                                                  |
| -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                        | *string*                                                                                     | :heavy_check_mark:                                                                           | Service account email for management roles                                                   |
| `platform`                                                                                   | [models.ManagerHeartbeatRequestPlatformGcp](../models/managerheartbeatrequestplatformgcp.md) | :heavy_check_mark:                                                                           | N/A                                                                                          |