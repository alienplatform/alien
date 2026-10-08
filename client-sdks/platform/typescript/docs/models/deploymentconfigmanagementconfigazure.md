# DeploymentConfigManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { DeploymentConfigManagementConfigAzure } from "@alienplatform/platform-api/models";

let value: DeploymentConfigManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `managingTenantId`                                                                 | *string*                                                                           | :heavy_check_mark:                                                                 | The managing Azure Tenant ID for cross-tenant access                               |
| `oidcIssuer`                                                                       | *string*                                                                           | :heavy_check_mark:                                                                 | OIDC issuer URL trusted by the target-side managed identity.                       |
| `oidcSubject`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | OIDC subject claim trusted by the target-side managed identity.                    |
| `platform`                                                                         | [models.DeploymentConfigPlatformAzure](../models/deploymentconfigplatformazure.md) | :heavy_check_mark:                                                                 | N/A                                                                                |