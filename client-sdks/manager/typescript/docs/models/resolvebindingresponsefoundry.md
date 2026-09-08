# ResolveBindingResponseFoundry

Azure AI Foundry and a Cognitive Services access token.

## Example Usage

```typescript
import { ResolveBindingResponseFoundry } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseFoundry = {
  binding: {
    account: "76435551",
    endpoint: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1750818839042",
  resourceId: "<id>",
  service: "foundry",
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                       | [models.RemoteAzureFoundryAiBinding](../models/remoteazurefoundryaibinding.md)                                                  | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `clientConfig`                                                                                                                  | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                          | :heavy_check_mark:                                                                                                              | Response-safe Azure client configuration containing one storage-audience<br/>access token for the stack's Remote Bindings identity. |
| `expiresAt`                                                                                                                     | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `resourceId`                                                                                                                    | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `service`                                                                                                                       | *"foundry"*                                                                                                                     | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |