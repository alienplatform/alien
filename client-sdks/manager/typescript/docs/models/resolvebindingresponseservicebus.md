# ResolveBindingResponseServicebus

Send-only Servicebus queue and a short-lived credential lease.

## Example Usage

```typescript
import { ResolveBindingResponseServicebus } from "@alienplatform/manager-api/models";

let value: ResolveBindingResponseServicebus = {
  binding: {
    namespace: "<value>",
    queueName: "<value>",
  },
  clientConfig: {
    credentials: {
      token: "<value>",
      type: "accessToken",
    },
    subscriptionId: "<id>",
    tenantId: "<id>",
  },
  expiresAt: "1756357009080",
  service: "servicebus",
};
```

## Fields

| Field                                                                                                                           | Type                                                                                                                            | Required                                                                                                                        | Description                                                                                                                     |
| ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| `binding`                                                                                                                       | [models.RemoteServiceBusQueueBinding](../models/remoteservicebusqueuebinding.md)                                                | :heavy_check_mark:                                                                                                              | Concrete send-only queue topology returned to remote clients.                                                                   |
| `clientConfig`                                                                                                                  | [models.RemoteAzureClientConfig](../models/remoteazureclientconfig.md)                                                          | :heavy_check_mark:                                                                                                              | Response-safe Azure client configuration containing one storage-audience<br/>access token for the stack's Remote Bindings identity. |
| `expiresAt`                                                                                                                     | *string*                                                                                                                        | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |
| `service`                                                                                                                       | *"servicebus"*                                                                                                                  | :heavy_check_mark:                                                                                                              | N/A                                                                                                                             |