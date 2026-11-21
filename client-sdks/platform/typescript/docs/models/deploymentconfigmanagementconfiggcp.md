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
| `federationTrust`                                                              | *models.DeploymentConfigFederationTrustUnion*                                  | :heavy_minus_sign:                                                             | N/A                                                                            |
| `serviceAccountEmail`                                                          | *string*                                                                       | :heavy_check_mark:                                                             | Linked source account metadata; never a direct target trust grant.             |
| `platform`                                                                     | [models.DeploymentConfigPlatformGcp](../models/deploymentconfigplatformgcp.md) | :heavy_check_mark:                                                             | N/A                                                                            |