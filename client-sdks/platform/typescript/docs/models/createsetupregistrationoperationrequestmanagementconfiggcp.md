# CreateSetupRegistrationOperationRequestManagementConfigGcp

GCP management configuration extracted from stack settings

## Example Usage

```typescript
import { CreateSetupRegistrationOperationRequestManagementConfigGcp } from "@alienplatform/platform-api/models";

let value: CreateSetupRegistrationOperationRequestManagementConfigGcp = {
  serviceAccountEmail: "<value>",
  platform: "gcp",
};
```

## Fields

| Field                                                                                                                        | Type                                                                                                                         | Required                                                                                                                     | Description                                                                                                                  |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `serviceAccountEmail`                                                                                                        | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Service account email for management roles                                                                                   |
| `platform`                                                                                                                   | [models.CreateSetupRegistrationOperationRequestPlatformGcp](../models/createsetupregistrationoperationrequestplatformgcp.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |