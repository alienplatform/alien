# AzureCredentialsManagedIdentity

Azure Managed Identity (Container Apps / App Service)
Uses IDENTITY_ENDPOINT + IDENTITY_HEADER injected by the platform

## Example Usage

```typescript
import { AzureCredentialsManagedIdentity } from "@alienplatform/manager-api/models";

let value: AzureCredentialsManagedIdentity = {
  clientId: "<id>",
  identityEndpoint: "<value>",
  identityHeader: "<value>",
  type: "managedIdentity",
};
```

## Fields

| Field                                                      | Type                                                       | Required                                                   | Description                                                |
| ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- | ---------------------------------------------------------- |
| `clientId`                                                 | *string*                                                   | :heavy_check_mark:                                         | The client ID of the user-assigned managed identity        |
| `identityEndpoint`                                         | *string*                                                   | :heavy_check_mark:                                         | The identity endpoint URL (from IDENTITY_ENDPOINT env var) |
| `identityHeader`                                           | *string*                                                   | :heavy_check_mark:                                         | The identity header secret (from IDENTITY_HEADER env var)  |
| `type`                                                     | *"managedIdentity"*                                        | :heavy_check_mark:                                         | N/A                                                        |