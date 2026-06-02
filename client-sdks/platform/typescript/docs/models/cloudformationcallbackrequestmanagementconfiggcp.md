# CloudFormationCallbackRequestManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { CloudFormationCallbackRequestManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: CloudFormationCallbackRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                                    | *string*                                                                                                 | :heavy_check_mark:                                                                                       | Service account email for management roles                                                               |
| `platform`                                                                                               | [models.CloudFormationCallbackRequestPlatformGcp](../models/cloudformationcallbackrequestplatformgcp.md) | :heavy_check_mark:                                                                                       | N/A                                                                                                      |