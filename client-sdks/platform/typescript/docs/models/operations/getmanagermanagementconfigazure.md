# GetManagerManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { GetManagerManagementConfigAzure } from "@alienplatform/platform-api/models/operations";

let value: GetManagerManagementConfigAzure = {
  managingTenantId: "<id>",
  oidcIssuer: "<value>",
  oidcSubject: "<value>",
  platform: "azure",
};
```

## Fields

| Field                                                           | Type                                                            | Required                                                        | Description                                                     |
| --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------- |
| `managingTenantId`                                              | *string*                                                        | :heavy_check_mark:                                              | The managing Azure Tenant ID for cross-tenant access            |
| `oidcIssuer`                                                    | *string*                                                        | :heavy_check_mark:                                              | OIDC issuer URL trusted by the target-side managed identity.    |
| `oidcSubject`                                                   | *string*                                                        | :heavy_check_mark:                                              | OIDC subject claim trusted by the target-side managed identity. |
| `platform`                                                      | *"azure"*                                                       | :heavy_check_mark:                                              | N/A                                                             |