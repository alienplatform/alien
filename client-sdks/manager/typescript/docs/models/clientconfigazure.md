# ClientConfigAzure

Azure client configuration

## Example Usage

```typescript
import { ClientConfigAzure } from "@alienplatform/manager-api/models";

let value: ClientConfigAzure = {
  credentials: {
    tokens: {
      "key": "<value>",
      "key1": "<value>",
    },
    type: "scopedAccessTokens",
  },
  subscriptionId: "<id>",
  tenantId: "<id>",
  platform: "azure",
};
```

## Fields

| Field                                                                      | Type                                                                       | Required                                                                   | Description                                                                |
| -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `credentials`                                                              | *models.AzureCredentials*                                                  | :heavy_check_mark:                                                         | Represents Azure authentication credentials                                |
| `region`                                                                   | *string*                                                                   | :heavy_minus_sign:                                                         | Azure region for resources.                                                |
| `serviceOverrides`                                                         | [models.AzureServiceOverrides](../models/azureserviceoverrides.md)         | :heavy_minus_sign:                                                         | N/A                                                                        |
| `subscriptionId`                                                           | *string*                                                                   | :heavy_check_mark:                                                         | The Azure Subscription ID where resources will be deployed.                |
| `tenantId`                                                                 | *string*                                                                   | :heavy_check_mark:                                                         | The customer's Azure Tenant ID.                                            |
| `platform`                                                                 | [models.ClientConfigPlatformAzure](../models/clientconfigplatformazure.md) | :heavy_check_mark:                                                         | N/A                                                                        |