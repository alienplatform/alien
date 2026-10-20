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
| `federationTrust`                                                                                                            | *models.CreateSetupRegistrationOperationRequestFederationTrustUnion*                                                         | :heavy_minus_sign:                                                                                                           | N/A                                                                                                                          |
| `serviceAccountEmail`                                                                                                        | *string*                                                                                                                     | :heavy_check_mark:                                                                                                           | Linked source account metadata; never a direct target trust grant.                                                           |
| `platform`                                                                                                                   | [models.CreateSetupRegistrationOperationRequestPlatformGcp](../models/createsetupregistrationoperationrequestplatformgcp.md) | :heavy_check_mark:                                                                                                           | N/A                                                                                                                          |