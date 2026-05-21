# GetManagerManagementConfigAzure

Azure management configuration extracted from stack settings

## Example Usage

```typescript
import { GetManagerManagementConfigAzure } from "@alienplatform/platform-api/models/operations";

let value: GetManagerManagementConfigAzure = {
  managingTenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                 | Type                                                                  | Required                                                              | Description                                                           |
| --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------- |
| `managementPrincipalId`                                               | *string*                                                              | :heavy_minus_sign:                                                    | Management service principal object ID for local development fallback |
| `managingTenantId`                                                    | *string*                                                              | :heavy_check_mark:                                                    | The managing Azure Tenant ID for cross-tenant access                  |
| `oidcIssuer`                                                          | *string*                                                              | :heavy_minus_sign:                                                    | OIDC issuer URL for federated identity credential creation            |
| `oidcSubject`                                                         | *string*                                                              | :heavy_minus_sign:                                                    | OIDC subject claim for federated identity credential creation         |
| `platform`                                                            | *"azure"*                                                             | :heavy_check_mark:                                                    | N/A                                                                   |