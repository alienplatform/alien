# ManagerManagementConfigsAzure

## Example Usage

```typescript
import { ManagerManagementConfigsAzure } from "@alienplatform/platform-api/models";

let value: ManagerManagementConfigsAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `managingTenantId`                                                                                 | *string*                                                                                           | :heavy_check_mark:                                                                                 | The managing Azure Tenant ID for cross-tenant access                                               |
| `oidcIssuer`                                                                                       | *string*                                                                                           | :heavy_check_mark:                                                                                 | OIDC issuer URL trusted by the target-side managed identity.                                       |
| `oidcSubject`                                                                                      | *string*                                                                                           | :heavy_check_mark:                                                                                 | OIDC subject claim trusted by the target-side managed identity.                                    |
| `platform`                                                                                         | [models.ManagerManagementConfigsPlatformAzure](../models/managermanagementconfigsplatformazure.md) | :heavy_check_mark:                                                                                 | N/A                                                                                                |